# aramkit-cache

The statistics service behind [ARAM Mayhem Tracker](../README.md). A crawler fetches
[aramkit](https://aramkit.com)'s data once per data version, one document every five seconds, and the
service stores it in SQLite and serves payloads shaped for the app. The app never contacts aramkit
itself, and the service never does on a request, so however many people use it, aramkit sees one slow
and steady client.

TypeScript on Node 24, with Fastify, better-sqlite3 and pino.

## This folder stands alone

It has its own pnpm root (`pnpm-workspace.yaml` here keeps pnpm from walking up to the repository's
workspace), its own lockfile, and **no dependency on anything above this folder**. Railway builds it
from this folder alone.

## Run it locally

```sh
cd service
pnpm install
PORT=8123 DATABASE_PATH=./local.db pnpm dev     # restarts on every source change
curl -s localhost:8123/v1/health
```

Node runs the TypeScript sources directly, so `pnpm dev` needs no build. The database file is
created on first run and the migrations apply on their own. A fresh database answers
`{"ok":true,"patch":null,…}` and every data route answers 503 until a crawl has completed. Run one
against the local service (about 15 minutes for a whole version, at the real pace):

```sh
ADMIN_TOKEN=dev PORT=8123 DATABASE_PATH=./local.db pnpm dev                     # the service needs the token too
SERVICE_URL=http://127.0.0.1:8123 ADMIN_TOKEN=dev node src/scripts/crawl.ts    # in another shell
```

To point the desktop app at a local service, create `%LOCALAPPDATA%\fr.sygnano.aram-mayhem-tracker\tuning.json`
containing `{ "serviceBase": "http://127.0.0.1:8123" }` and restart the app. Delete the file
afterwards: the app reads it at every start.

```sh
pnpm typecheck          # tsc --noEmit
pnpm test               # vitest
pnpm lint               # biome check; `pnpm format` applies the fixes
pnpm build              # compiles to dist/, which `pnpm start` and the image run
```

## Endpoints

| Route | What it answers |
|---|---|
| `GET /v1/health` | `{ ok, patch, dataDate, ageSeconds, championsCached, crawling, uptimeSeconds }`. `crawling` names a newer version still being crawled. Never rate-limited |
| `GET /v1/patch` | The data version served. 503 until a crawl has completed |
| `GET /v1/dataset` | **What the app downloads** (D-090): every champion's bundle as `/v1/champion` sends it, keyed by id, plus the champion table and the whole anvil rankings document. About 113 MB of JSON, sent gzipped (about 13 MB) as built once per data version and rankings save. Send `If-None-Match` with the `ETag` you hold to get a 304 |
| `GET /v1/champion?champion=157` | Everything about one champion: the ranked augment pool for each rarity at each of the four stages, the build archetypes, and the champion's stat anvil ranking group. What app 1.1 and earlier ask for |
| `GET /v1/champions` | Every champion's rank, tier, win rate and pick rate, with `poolSize`. What app 1.1 and earlier ask for |
| `GET /v1/anvils?locale=en_us` | The stat anvil shards, named in the given language |
| `GET /v1/damage?champions=157,99` | Each champion's physical, magic and true damage split, and the split of them together. Asked about the enemy team at game start |
| `GET /v1/pool`, `GET /v1/build`, `GET /v1/offer` | Parts of `/v1/champion`, kept in the API; the app no longer calls them |
| `GET /v1/anvil-rankings` | The stat anvil rankings document, as the editor loads it |
| `PUT /v1/anvil-rankings` | Saves a new rankings document. Needs `Authorization: Bearer $ADMIN_TOKEN`, and is validated in full first |
| `GET /admin` | The rankings editor. Never rate-limited |
| `GET /admin/crawl`, `POST /admin/crawl/versions`, `PUT /admin/crawl/doc`, `PUT /admin/crawl/absent` | The crawler's routes, documented in [src/routes/crawl.ts](src/routes/crawl.ts). Need `Authorization: Bearer $ADMIN_TOKEN`. Never rate-limited |

Errors are JSON, `{ "error": "…", "retryAfter": null }`, with a `Retry-After` header on 429 and 503.
Champion ids that aramkit does not rank on the current patch are refused with a 404, and so is a
champion it ranks but has no details for (the crawler records those).

## Configuration

Everything comes from the environment, and nothing is required.

| Variable | Default | Notes |
|---|---|---|
| `PORT` | `8080` | Railway sets it |
| `DATABASE_PATH` | `/data/cache.db` | On Railway, a path on the mounted volume |
| `ADMIN_TOKEN` | *unset* | Needed to **save** the anvil rankings and for **the crawler** to send anything. Unset means both are refused, so a deploy that forgets it is locked rather than open |
| `REQUESTS_PER_SECOND_PER_IP` | `1` | Sustained requests per second from one client address. `0` turns the per-address limit off |
| `BURST_PER_IP` | `20` | Requests one address may make at once before the sustained rate applies |
| `REQUESTS_PER_SECOND` | `200` | A ceiling across all clients together, with bursts of twice that. `0` turns it off |
| `UPSTREAM_USER_AGENT` | `aram-mayhem-tracker/<version> (+https://github.com/Sygnano/aram-mayhem-tracker)` | Sent to aramkit and CommunityDragon, so they can see who is calling and how to reach us |
| `UPSTREAM_CONCURRENCY` | `4` | Most CommunityDragon fetches at once |
| `ARAMKIT_BASE` | `https://data.aramkit.com` | Read by the crawler only. Overridable for tests |
| `CDRAGON_BASE` | `https://raw.communitydragon.org` | Overridable for tests |
| `LOG_LEVEL` | `info` | `fatal`, `error`, `warn`, `info`, `debug`, `trace` or `silent` |

The crawler reads the same variables, plus its own:

| Variable | Default | Notes |
|---|---|---|
| `SERVICE_URL` | *required* | The web service, on Railway's private network: `http://<service>.railway.internal:<PORT>` |
| `ADMIN_TOKEN` | *required* | The web service's token |
| `CRAWL_INTERVAL_SECS` | `5` | Time between two requests to aramkit |
| `CRAWL_MAX_FAILURES` | `5` | Failures in a row before a run gives up; the next run resumes where it stopped |

### Who the rate limit counts

The limit is per client address. Behind Railway, connections come from Railway's edge proxy, so the
client is read from the leftmost `X-Forwarded-For` entry, which Railway's edge writes after stripping
any value the client sent. That header is believed only when the connection comes from Railway's
proxy range, `100.0.0.0/8`. Anywhere else the socket's own address counts and the header is ignored.
`X-Real-IP` is not used: Railway's CDN puts its own address there.

## Deploy on Railway

1. Create a service from this repository and set its **root directory to `service`**. Railway builds
   it with Railpack, which reads `package.json`: pnpm from `packageManager`, then `pnpm build`, then
   `pnpm start`. Everything else is set in the service's dashboard settings, not in a file: the
   health check path `/v1/health` with a 30-second timeout, and the restart policy On Failure with 10
   retries.
2. Add a **volume mounted at `/data`**. Without it the cache is lost on every deploy and everything is
   crawled from aramkit again, which is the load this service exists to avoid.
3. Set `ADMIN_TOKEN`. The crawler needs it, and so does the `/admin` editor to save.
4. Generate a public domain and check `https://<domain>/v1/health`.
5. **The crawler**: create a second service from the same repository and root directory, with no
   volume and no public domain, and no config file path. In its Deploy settings, set the start
   command to `pnpm crawl`, the cron schedule to `0 * * * *` (hourly), no health check path and the
   restart policy to Never. Set `SERVICE_URL` and `ADMIN_TOKEN`. Railway runs it on schedule, skips a
   run while the last one is still going, and the script exits when it is done. A new data version
   takes about 15 minutes to crawl; until then the previous one is served, and a fresh deploy answers
   503 until its first crawl completes.

Deploy the service **before** releasing an app version that depends on a new route: an app that gets
a 404 for a route has nothing to fall back on.

### The anvil rankings are not a cache

Everything else in the database can be crawled again. **The anvil rankings cannot**: they are what
was authored in `/admin`, and the volume is the only copy.

- A save that fails validation is refused, and the previous one stays.
- Saves are appended, never overwritten: the `anvil_rankings` table keeps the last 200, and the
  newest is the current document. To restore one, read its `body`
  (`SELECT id, saved_at, body FROM anvil_rankings ORDER BY id DESC`) and `PUT` it back.
- The editor's **Download JSON** button keeps a copy outside Railway.

Deleting the volume deletes the rankings.

### The volume permission trap

Railway mounts volumes owned by `root` at runtime, over whatever the image prepared. A service running
as an unprivileged user then cannot create its database, and SQLite only says `unable to open
database file`. The service runs as whatever user Railpack's image starts it as, so if that is not
root, the write probe at startup fails and names the directory and the user id. Railway's documented
answer is to set `RAILWAY_RUN_UID=0` on the service.
