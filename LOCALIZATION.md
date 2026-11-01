# Interface and Brazilian Portuguese localization

The interface supports English and Brazilian Portuguese without an additional runtime dependency.
Settings → Language offers `auto`, `en` and `pt-BR`. The existing `config.json` stores the choice;
older files default to `auto`. Automatic mode uses the system's preferred language (Portuguese maps
to pt-BR; unsupported languages fall back to English). Both webviews subscribe to the backend
`language-changed` event. Native tray labels use the same catalog and Windows locale detection.
Application text lives in `apps/desktop/src/locales/pt-BR.json`; English source keys provide the
fallback. Named parameters allow translated sentences to change word order. Dates and displayed
decimal numbers follow the interface locale. Existing French item-set labels are preserved.

## Official game names and stable identifiers

The interface choice does not override the League client language. OCR must match the words that
are actually on the cards. The existing CommunityDragon loader uses `pt_br` for `pt_BR`/`pt-BR`,
caches each locale separately by patch, and loads the official `nameTRA` augment names. Statistics
continue to join those names by augment id, with the existing ranking unchanged.

Champion summaries now load in the same game locale. Alias and Riot champion id remain the lookup
keys; only display names change. The retry path checks patch **and locale** before applying names,
so a late response cannot replace names for another client language.

Items remain numeric ids in the generated sets. The League client resolves their official names;
the app supplies only section labels and build archetype descriptions. Brazilian Portuguese adds
translations for these application-owned labels, preserving item order and existing English/French
behavior. The existing anvil service already accepts `pt_br` and derives localized shard names
from CommunityDragon; neither its code nor its deployment settings were changed.

No game name is manually translated in the interface catalog. If a localized augment is absent,
the existing statistics path retains the upstream name. Missing localized champion summaries fall
back to the official default summary, preserving champion ids and the original lookup behavior.
If default names are used, localized names are attempted on the next full static-data load (for
example, after restarting); a completely missing index is still retried separately. The app does
not fabricate a translation. Unknown build archetypes retain their existing key-based
fallback, and the item-set mode suffix retains `ARAM Mayhem` when no existing localized suffix is
available. Technical ids, raw OCR readings, debug structures and third-party/Windows error details
stay verbatim. Application-owned diagnostic wrappers are translated at the presentation boundary;
raw logs stay unchanged for issue reports. New diagnostic message formats need corresponding
catalog entries.

## OCR scope and evidence

The bundled PaddleOCR model, card capture, thresholds and name-matching algorithm are unchanged.
The matcher already applies NFKD normalization, removes combining accents and whitespace, and
handles punctuation and existing OCR glyph confusions. Tests add Portuguese accents, cedilla,
nonbreaking spaces and official names with their original ids. These test **text matching**, not
the accuracy of image recognition in a real Portuguese game.

`crates/mayhem-vision/assets/ocr/pt-br-names.json` is a six-row test-only subset of
[CommunityDragon 16.21 cherry-augments.json](https://raw.communitydragon.org/16.21/plugins/rcp-be-lol-game-data/global/pt_br/v1/cherry-augments.json),
retrieved on 2026-11-01. Only `id`, `nameTRA` and `rarity` were retained. It is never a runtime
vocabulary: runtime names always come through the existing CommunityDragon integration. The
localized champion cache test uses the official `Nunu e Willump` / alias `Nunu` / id `20` from
[champion-summary.json](https://raw.communitydragon.org/16.21/plugins/rcp-be-lol-game-data/global/pt_br/v1/champion-summary.json).

## Validation

Run the desktop checks used by CI, plus the localization tests (also wired into the desktop
CI job), on Windows with the Rust toolchain pinned by `rust-toolchain.toml`:

```sh
pnpm install --frozen-lockfile
pnpm --filter desktop test:i18n
pnpm typecheck
pnpm --filter desktop build
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Coverage includes English fallback, catalog keys and interpolation, language notifications and
a stale initial-settings response, Brazilian decimal formatting, saved-language round trips,
existing English/French matching and screenshot fixtures, official Portuguese names, localized
champion caches with an English fallback, and unchanged item ids and purchase order. Existing
asset-generation and measurement tools remain ignored by the test suite.

The separate Windows validation build has been launched successfully by the contributor. This
does not establish complete Portuguese OCR accuracy in a live match. No Portuguese in-game
image fixture has been validated yet. The OCR regression added here tests official name matching
and normalization; the existing model, capture geometry and matching thresholds are unchanged.

## Manual checks still required

1. Verify the language selector, persistence after restart, native tray labels, and live switching
   between the companion and overlay windows. Check automatic mode on Portuguese and
   non-Portuguese Windows. Use a separate development environment to avoid changing an installed
   copy's settings.
2. Set League to Portuguese (Brazil) and use borderless/windowed mode. In ARAM Mayhem, inspect
   offers at levels 1, 7, 11 and 15, including accents, long names and golden rerolls. Compare raw
   OCR output in diagnostics to the actual cards at several resolutions and Windows DPI scales.
   Collect Portuguese screenshots for future image-level regression fixtures.
3. Check anvil titles and decimal commas, champion and bench information, item ids/order, cached
   statistics and update messages. Keep the original README's item-set replacement warning in
   mind when testing item-set writes. Record missing official names rather than inventing aliases.
4. Recheck English and French clients in a real match. Their existing automated matching and
   screenshot coverage is preserved; no new live-client validation is claimed.

The application identity, version, updater, Windows autostart, statistics service, MIT license,
third-party notices, copyright and original developer credits are unchanged.
