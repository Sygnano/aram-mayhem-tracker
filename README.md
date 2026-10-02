# ARAM Mayhem Tracker

A companion app and in-game overlay for **League of Legends: ARAM Mayhem**, for Windows.

It shows how well each augment, champion and stat anvil performs for the champion you are playing,
right where you choose them, and writes recommended item sets into the client when you lock in.

- **Augment offers**: each card gets its tier, its win-rate difference for your champion and its rank
  in the pool, with the numbers per stage. The full ranked list for that rarity can sit in the top
  right corner of the screen.
- **Champ select**: tier, rank, win rate and pick rate on the pick cards, the bench and your team.
  Click a bench block to swap to that champion.
- **Stat anvils**: a rank label on each shard card. When Armor and Magic Resist are tied, the enemy
  team's damage split decides between them.
- **Item sets**: one per build type (Crit, Bruiser, AP…), written into the client when your champion
  is locked in.
- **Quality of life**: accept the ready check automatically, and set your chat status (Online, Away,
  Offline) from the app.

The statistics come from [aramkit](https://aramkit.com), refreshed with each patch.

## Before you install

- **Item sets replace every item page on your account.** With *Manage item sets* on, which is the
  default, locking in a champion in a Mayhem queue replaces the whole collection: item pages you made
  by hand are deleted, with no undo, and the client's own recommended pages are removed too. Untick
  *Manage item sets* before your first Mayhem game if you want to keep yours.
- **Borderless or windowed mode only.** The overlay cannot be drawn over exclusive fullscreen. Set
  the game's window mode to *Borderless* in its video settings.
- **Supported client languages.** Augment and anvil cards are read off the screen with a text
  recognition model that knows Latin scripts and Chinese; English and French are confirmed. On
  Korean, Japanese, Russian, Greek, Thai and other non-Latin clients the in-game cards are not read
  yet. Champ select and item sets work in every language. Item set titles are in English or French.
- **The installer is not code-signed yet**, so Windows SmartScreen warns before the first run.

## Install

1. Download `ARAM Mayhem Tracker_<version>_x64-setup.exe` from the
   [latest release](https://github.com/Sygnano/aram-mayhem-tracker/releases/latest).
2. Run it. If SmartScreen says *Windows protected your PC*, choose **More info**, then **Run anyway**.
3. Open **ARAM Mayhem Tracker** from the Start menu.

Requirements: Windows 10 or 11, 64-bit. The installer adds Microsoft Edge WebView2 if it is missing;
Windows 11 already has it. Nothing else is needed: the text recognition model ships inside the app.

Only one copy runs at a time. Opening the app again brings up the window of the copy already running.

**Updates**: the *Updates* section of the companion window checks for a newer version, at startup or
when you click *Check for updates*. If there is one, *Download update* downloads it, checks its
signature, installs it and restarts the app. Nothing is installed without that click.

### Portable copy

To run the app without installing it, download `ARAM Mayhem Tracker_<version>_x64-portable.zip`
from the same release and extract it to a folder you own, such as one under *Documents* or on a USB
drive, then run `aram-mayhem-tracker.exe`. The `portable` file next to it is what makes it portable:
settings, logs and the game data cache are kept in a `data` folder beside the executable, so the
folder can be moved or copied whole. If that folder cannot be written (a zip opened in place, or a
folder under *Program Files*), the app warns and uses the installed copy's folders instead.

A portable copy needs Microsoft Edge WebView2, which Windows 11 has and which the zip cannot add.
WebView2 keeps its own browser data under `%LOCALAPPDATA%\dev.syg.aram-mayhem-tracker`. A portable
copy checks for updates like an installed one, but *Open release page* replaces *Download update*:
download the new zip and replace the folder's files with it, keeping `data`. *Start with Windows
minimized* starts the copy from where it is, so untick it before moving the folder.

To uninstall, use *Settings → Apps → Installed apps* in Windows. Your settings, logs and cache stay
in `%LOCALAPPDATA%\dev.syg.aram-mayhem-tracker`; delete that folder to remove everything. To remove
a portable copy, delete its folder, and that same `%LOCALAPPDATA%` folder for WebView2's data.

## Using it

Start the app, then League. The companion window says **OK** once it has found the League client
and loaded the game data; there is nothing to configure. In a Mayhem game the overlay appears over
the game window on its own, and over the client during champ select.

| Option | What it does |
|---|---|
| **Enabled** | Shows the overlay. Untick it to hide the overlay everywhere; nothing else stops |
| **Simple mode** | Tier badges only, no numbers |
| **Show augment list** | The ranked list for the offer's rarity, in the top right corner |
| **Manage item sets** | Writes the item sets on lock-in. Replaces every item page on the account (see above) |
| **Auto accept** | Accepts the ready check one second after a game is found. Off by default |
| **Chat status** | Sets Online, Away or Offline in the client |
| **Keep in tray** | Closing the window keeps the app running in the tray |
| **Start with Windows minimized** | Starts the app when you sign in to Windows, out of the way |
| **Check updates on startup** | Looks for a newer version each time the app starts. On by default |

The gear in the companion window opens a diagnostics screen: what the screen reader sees, where
the overlay is placed, and the path of the log file.

## How it works

Everything it reads comes from your own machine or from public sources:

- **The League client's local API** (the LCU, on `127.0.0.1`) for the gameflow phase, the queue,
  champ select, the ready check, the chat status and item sets. The app finds it through the
  client's `lockfile`, the same way the client's own tools do.
- **The game's Live Client Data API** (`127.0.0.1:2999`) for your champion, level and the teams.
- **Your screen**, for the offered augments and anvil shards. Riot does not publish which augments
  are offered, so the app takes small captures of the card area while an offer is up and reads the
  titles with a bundled text recognition model. Nothing is captured outside a Mayhem game.
- **[CommunityDragon](https://www.communitydragon.org)** for augment names and the Mayhem augment pool
  in your client's language.
- **Our statistics service**, which caches aramkit's data per patch and serves only what the app
  needs.

The app never injects anything into the game or the client, never reads their memory, and does not
use the Riot Games web API. It shows no enemy cooldowns or timers.

**What leaves your machine**: requests to CommunityDragon for the game data, and to our service with
champion ids (yours, and the enemy team's once a game starts) and your client's language. No account
name, summoner id or other personal data is sent. Your IP address is visible to the service, as to
any website, and is used only to rate-limit requests. If the service cannot be reached, the app uses
the last answers it stored.

## Troubleshooting

- **The overlay does not appear in game**: check that the game runs in *Borderless* mode and that
  *Enabled* is ticked. In game, the overlay only shows in ARAM Mayhem; the champ-select blocks also
  show in plain ARAM.
- **The augment cards are not recognised**: open the diagnostics screen (the gear) during an offer.
  It shows whether the game window and the cards were found, and what was read.
- **Something else**: the log is at `%LOCALAPPDATA%\dev.syg.aram-mayhem-tracker\logs\mayhem.log`.
  Please attach it to an [issue](https://github.com/Sygnano/aram-mayhem-tracker/issues).

## Building from source

Requirements: Rust through [rustup](https://rustup.rs) (the pinned toolchain in
`rust-toolchain.toml` installs itself), Node 22 or later with pnpm, and WebView2.

```sh
pnpm install
pnpm dev                        # the app with hot reload
pnpm build                      # the NSIS installer, under target/release/bundle/nsis/
cargo test --workspace          # all Rust tests (run `pnpm --filter desktop build` once first)
```

| Folder | What it holds |
|---|---|
| `apps/desktop` | The Tauri app: `src-tauri` is the Rust backend, `src` the React windows |
| `crates/mayhem-core` | Game logic: queues, the game clock, augment offers, layouts, anvil ranks |
| `crates/mayhem-vision` | Screen reading: card detection, text recognition, name matching |
| `crates/league-api` | The Live Client Data and LCU clients |
| `crates/static-data` | CommunityDragon data, cached per patch |
| `crates/aramkit-client` | The client for our statistics service, with an offline copy |
| `service` | The statistics service (TypeScript, deployed on Railway). See [service/README.md](service/README.md) |

### Releasing

Releases are built by GitHub Actions (`.github/workflows/release.yml`) from the version in the code.
There is no tag to push by hand.

1. Run `pnpm bump <version>` (for example `pnpm bump 1.0.1`), then commit and push to `main`. It sets
   the version in `apps/desktop/src-tauri/tauri.conf.json`, `apps/desktop/package.json`,
   `apps/desktop/src-tauri/Cargo.toml` and `Cargo.lock`. The workflow fails if the first three differ,
   and CI fails on a `Cargo.lock` left behind, since it builds with `--locked`.
2. When that version has no release yet, draft or published, the workflow runs CI, builds the
   installer, signs the update files and creates a **draft** release `v<version>` with the installer,
   its signature, `latest.json` and the portable zip. Any other push to `main` stops after the version check.
3. Review the draft on GitHub, write the notes, then **publish** it. GitHub creates the tag on the
   commit that was built, and installed copies see the update from then on.

To rebuild a version, delete its draft and run the workflow from the Actions tab (*Run workflow*). A
published version is never rebuilt: bump the version instead.

The update files are signed with the project's updater key. The workflow reads it from two
repository secrets, `TAURI_SIGNING_PRIVATE_KEY` (the contents of the private key file) and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. A local `pnpm build` needs the same two variables in the
environment.

## License

MIT, see [LICENSE](LICENSE). The app also ships work by others under their own licenses, listed in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md), including the PaddleOCR text recognition model
(Apache-2.0).

ARAM Mayhem Tracker isn't endorsed by Riot Games and doesn't reflect the views or opinions of Riot
Games or anyone officially involved in producing or managing Riot Games properties. Riot Games, and
all associated properties are trademarks or registered trademarks of Riot Games, Inc.
