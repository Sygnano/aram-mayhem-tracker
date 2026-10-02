# aramkit-cache

The statistics service behind [ARAM Mayhem Tracker](../README.md). It fetches
[aramkit](https://aramkit.com)'s data once per patch, stores it in SQLite, and serves small payloads
shaped for the app. The app never contacts aramkit itself, so however many people use it, aramkit
sees one client.

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
`{"ok":true,"patch":null,…,"championsCached":0}` for the first moments; the patch fills in once
aramkit's `versions.json` has been read.

To point the desktop app at a local service, create `%APPDATA%\dev.syg.aram-mayhem-tracker\tuning.json`
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
| `GET /v1/health` | `{ ok, patch, dataDate, ageSeconds, championsCached, uptimeSeconds }`. Never rate-limited |
| `GET /v1/patch` | The current patch and data date. 503 until `versions.json` has been read once |
| `GET /v1/champion?champion=157` | Everything the app needs about one champion, in one response: the ranked augment pool for each rarity at each of the four stages, the build archetypes, and the champion's stat anvil ranking group. The app sends one per champion |
| `GET /v1/champions` | Every champion's rank, tier, win rate and pick rate, with `poolSize`. Fetched once per patch for champ select |
| `GET /v1/anvils?locale=en_us` | The stat anvil shards, named in the given language |
| `GET /v1/damage?champions=157,99` | Each champion's physical, magic and true damage split, and the split of them together. Asked about the enemy team at game start |
| `GET /v1/pool`, `GET /v1/build`, `GET /v1/offer` | Parts of `/v1/champion`, kept in the API; the app no longer calls them |
| `GET /v1/anvil-rankings` | The stat anvil rankings document, as the editor loads it |
| `PUT /v1/anvil-rankings` | Saves a new rankings document. Needs `Authorization: Bearer $ADMIN_TOKEN`, and is validated in full first |
| `GET /admin` | The rankings editor. Never rate-limited |

Errors are JSON, `{ "error": "…", "retryAfter": null }`, with a `Retry-After` header on 429 and 503.
Champion ids that aramkit does not rank on the current patch are refused with a 404 before anything
is fetched upstream.

## Configuration

Everything comes from the environment, and nothing is required.

| Variable | Default | Notes |
|---|---|---|
| `PORT` | `8080` | Railway sets it |
| `DATABASE_PATH` | `/data/cache.db` | On Railway, a path on the mounted volume |
| `ADMIN_TOKEN` | *unset* | Needed to **save** the anvil rankings. Unset means saving is refused, so a deploy that forgets it is locked rather than open |
| `REQUESTS_PER_SECOND_PER_IP` | `1` | Sustained requests per second from one client address. `0` turns the per-address limit off |
| `BURST_PER_IP` | `20` | Requests one address may make at once before the sustained rate applies |
| `REQUESTS_PER_SECOND` | `200` | A ceiling across all clients together, with bursts of twice that. `0` turns it off |
| `UPSTREAM_USER_AGENT` | `aram-mayhem-tracker/<version> (+https://github.com/Sygnano/aram-mayhem-tracker)` | Sent to aramkit and CommunityDragon, so they can see who is calling and how to reach us |
| `UPSTREAM_CONCURRENCY` | `4` | Most upstream fetches at once |
| `VERSIONS_POLL_SECS` | `21600` (6 h) | How often aramkit's `versions.json`, its only changing document, is read |
| `ARAMKIT_BASE` | `https://data.aramkit.com` | Overridable for tests |
| `CDRAGON_BASE` | `https://raw.communitydragon.org` | Overridable for tests |
| `LOG_LEVEL` | `info` | `fatal`, `error`, `warn`, `info`, `debug`, `trace` or `silent` |

### Who the rate limit counts

The limit is per client address. Behind Railway, connections come from Railway's edge proxy, so the
client is read from the leftmost `X-Forwarded-For` entry, which Railway's edge writes after stripping
any value the client sent. That header is believed only when the connection comes from Railway's
proxy range, `100.0.0.0/8`. Anywhere else the socket's own address counts and the header is ignored.
`X-Real-IP` is not used: Railway's CDN puts its own address there.

## Deploy on Railway

1. Create a service from this repository and set its **root directory to `service`**. `railway.json`
   selects the Dockerfile and points the health check at `/v1/health`.
2. Add a **volume mounted at `/data`**. Without it the cache is lost on every deploy and everything is
   fetched from aramkit again, which is the load this service exists to avoid.
3. Set `ADMIN_TOKEN` if the `/admin` editor should be able to save. Nothing else is required.
4. Generate a public domain and check `https://<domain>/v1/health`.

Deploy the service **before** releasing an app version that depends on a new route: an app that gets
a 404 for a route has nothing to fall back on.

### The anvil rankings are not a cache

Everything else in the database can be fetched again. **The anvil rankings cannot**: they are what
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
database file`. [docker-entrypoint.sh](docker-entrypoint.sh) starts as root, gives the database
directory to the service account, then drops to it, so the service itself never runs privileged. If a
recurrence happens, the service's own error names the directory and the user id.
