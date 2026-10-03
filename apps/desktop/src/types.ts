// Mirrors of the Rust types serialised to the frontends
// (src-tauri/src/engine/snapshot.rs, crates/mayhem-core, crates/mayhem-vision).
//
// Written by hand: nothing checks this file against the Rust side, so a field changed there has to
// be changed here in the same commit.

/** A card read off the screen. `rarity` is CommunityDragon's spelling (`kGold`). */
export interface AugmentCard { id: number; name: string; rarity: string }

/** Where one card is, normalised to the client area (0..1). Computed backend-side from the measured
 *  card layout, so the overlay never recomputes it. */
export interface CardAnchor {
  /** Horizontal centre of the card frame. */
  centerX: number;
  /** Where the panel's bottom edge goes. Panels grow upward from here, because the reroll button
   *  sits just below and anything growing down would cover it. */
  panelBottomY: number;
  /** Where a stat anvil label's bottom edge goes: inside the card, just above its bottom frame. */
  anvilBottomY: number;
  /** The panel's width: the card frame scaled by the configured `panelWidth`. */
  width: number;
}

/** Mirrors crates/aramkit-client/src/types.rs, which mirrors the service's wire contract. */
export interface StageInfo {
  /** aramkit's stage number, 1-4. */
  stage: number;
  /** The in-game level: 1, 7, 11, 15. */
  level: number;
  winRate: number;
  deltaPp: number;
  pickRate: number;
  sampleCount: number;
  lowSample: boolean;
}

export interface AugmentInfo {
  id: number;
  nameId: string | null;
  name: string | null;
  rarity: string | null;
  iconUrl: string | null;
  winRate: number | null;
  /** Percentage points against the baseline named by `deltaBasis`. Drives the ranking. */
  deltaPp: number | null;
  pickRate: number | null;
  augmentWinRate: number | null;
  sampleCount: number;
  tier: string | null;
  rank: number | null;
  source: "stage" | "champion" | "global" | "none";
  confidence: "high" | "low" | "fallback" | "none";
  /** Fewer than 200 games behind the number: show it, but warn. */
  lowSample: boolean;
  /** `champion` compares against this champion's own win rate; `global` against the 50% average. */
  deltaBasis: "champion" | "global" | "none";
  byStage: StageInfo[];
  /** Stages this augment can be offered at; an augment starting at level 11 lists [3, 4]. Empty
   *  when upstream does not say, which the UI treats as available everywhere. */
  availableStages: number[];
}

/** The three rarities an offer can be, in the service's spelling. */
export type Rarity = "silver" | "gold" | "prismatic";

/** An augment as the overlay draws it: the service's row plus what the engine decided about it
 *  (`engine/snapshot.rs` `RankedAugment`, `mayhem_core::ranking`). The overlay does none of this
 *  itself. */
export interface RankedAugment extends AugmentInfo {
  /** Which baseline the delta is against, and so which list the row is ranked in: this champion's
   *  own numbers, the all-champion fallback, or nothing. Never mixed in one ordering. */
  block: "champion" | "global" | "none";
  /** S to D from the delta; `?` when there is none. */
  grade: "S" | "A" | "B" | "C" | "D" | "?";
  /** The row's place within its block, among the augments of its own rarity; null with no data. */
  poolRank: number | null;
  /** What `poolRank` is out of. */
  poolRankOf: number | null;
}

/** The cards on screen, best first. */
export interface OfferStats {
  augments: RankedAugment[];
  /** Augment ids best first. */
  ranking: number[];
}

/** Every augment of one rarity for the champion, in ranked order. */
export interface PoolStats {
  rarity: Rarity;
  augments: RankedAugment[];
}

/** Which part of the champ-select screen a badge sits on. */
export type ChampSelectSurface = "pickCard" | "strip" | "ally";

/** One badge position on the champ-select screen.
 *
 *  Normalised to the League client window's client area, which the overlay covers exactly, so these
 *  are directly CSS percentages -- the same contract as `CardAnchor`. Positions come from a layout
 *  fitted to real screenshots, so they are exact rather than searched for. */
/** What a block shows: real aramkit numbers for that champion, by way of our caching service.
 *
 *  Rates are **percentages** here (57.78), not the fractions the service reports (0.5778). The
 *  conversion happens once, in the engine; nothing on this side multiplies again. */
export interface ChampionStats {
  /** aramkit's tier letter; null when they publish a row without one. */
  tier: string | null;
  /** aramkit's own rank, 1 is best. Their ordering is weighted rather than plain win-rate order,
   *  and it is what the tier beside it agrees with, so it is shown as given. */
  rank: number;
  /** What the rank is out of. Third of 173 and third of 5 are not the same claim. */
  poolSize: number;
  winRate: number;
  pickRate: number;
  /** Games behind these numbers. */
  sampleCount: number;
}

/** One statistics block.
 *
 *  There is deliberately no champion name: the client already prints it next to every one of these,
 *  and repeating it would spend the block's limited room saying something already on screen. */
export interface ChampSelectSlot {
  surface: ChampSelectSurface;
  index: number;
  championId: number;
  /** Null when the champion table has no row for this champion, or has not arrived yet. The block
   *  is still drawn -- on the bench it is the swap button -- with no figures in it. */
  stats: ChampionStats | null;
  /** Clicking this block swaps to that champion. Only the bench qualifies. */
  swappable: boolean;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Snapshot {
  client: {
    connected: boolean;
    phase: string | null;
    queueId: number | null;
    queueName: string | null;
    isMayhemQueue: boolean;
    locale: string | null;
    installDir: string | null;
    error: string | null;
  };
  game: {
    gameTime: number;
    gameMode: string;
    mapNumber: number;
    isMayhem: boolean;
    champion: string | null;
    level: number;
    isDead: boolean;
    respawnTimer: number;
    stagesUnlocked: number;
  } | null;
  augments: {
    offer: { stage: number; cards: (AugmentCard | null)[] } | null;
    stagesSeen: number;
    offerDue: boolean;
    ocrBlocker: string | null;
    /** Null until the game window size is known. */
    cardAnchors: [CardAnchor, CardAnchor, CardAnchor] | null;
    /** Every augment seen this game. They cannot be offered again. */
    seenAugmentIds: number[];
  };
  /** The backend believes the overlay should be on screen, loading screen included. */
  overlayActive: boolean;
  /** How the overlay draws, from the companion window's checkboxes. */
  options: { simpleMode: boolean; showAugmentList: boolean };
  /** Why the overlay window is, or is not, on screen. Every stage of that decision can fail quietly
   *  and they all look the same from outside — nothing drawn — so they are reported separately. */
  overlay: {
    target: "none" | "game" | "client";
    hostFound: boolean;
    hostFocused: boolean;
    visible: boolean;
  };
  /** The ARAM: Mayhem champ-select screen. Positions come from a fitted layout and everything else
   *  from the client's own champ-select session; nothing is read off the screen. */
  champSelect: {
    active: boolean;
    /** Champions on the big cards right now: 2 or 3 during `BAN_PICK`, 0 afterwards. */
    cardCount: number;
    /** Champions on the bench, which is what the available-champions strip shows. */
    benchSize: number;
    /** Champions the subset endpoint offers, drawn or not. With `cardCount` and `cardsSettled` this
     *  says why there are no card blocks: no offer, or an offer still animating in. */
    subsetSize: number;
    /** The cards have been up long enough for their entrance animation to have finished. */
    cardsSettled: boolean;
    /** `timer.adjustedTimeLeftInPhase`. Shown, never gated on. */
    timeLeftMs: number;
    /** `timer.totalTimeInPhase`, or 0 when the client omits it (it is undocumented). */
    phaseTotalMs: number;
    /** The champ-select phase the client reports: `PLANNING`, `BAN_PICK`, `FINALIZATION`. */
    phase: string;
    slots: ChampSelectSlot[];
    /** The session on screen is the last one that read successfully, not a fresh one. */
    stale: boolean;
    lastSwap: string | null;

    /** The aramkit patch the champion table is for; empty before it has been fetched. */
    dataPatch: string;
    /** The date aramkit built the data. */
    dataDate: string;
    /** `stale` when the table came off the on-disk copy because the service was unreachable. */
    freshness: "live" | "stale" | null;
    /** Why the champion table is missing or old, if it is. */
    rankingsError: string | null;
    /** Champions in the table. 0 means every block is drawn without figures. */
    rankedChampions: number;
  };

  /** Progress of the champ select prefetch: everything for the locked champion is downloaded
   *  before the game starts, so nothing in game waits on the network. */
  champion: {
    championId: number | null;
    ready: boolean;
    loading: boolean;
    poolsLoaded: number;
    poolsExpected: number;
    buildLoaded: boolean;
    error: string | null;
  };
  stats: {
    offer: OfferStats | null;
    /** The ranked list for this offer's rarity. */
    pool: PoolStats | null;
    /** The list one rarity up, present only while a card on screen is a golden reroll. */
    upgradedPool: PoolStats | null;
    /** `stale` means the service was unreachable and this is the stored copy. */
    freshness: "live" | "stale" | null;
    loading: boolean;
    error: string | null;
    /** An offer is on screen but the champion could not be identified. */
    championUnknown: boolean;
  };
  vision: {
    available: boolean;
    unavailableReason: string | null;
    ocrEngine: string | null;
    gameWindowFound: boolean;
    clientSize: [number, number] | null;
    samplesPerSecond: number;
    lastError: string | null;
    /** The reroll buttons: the first and cheapest gate, and the one a tooltip cannot cover. */
    rerolls: Reroll[];
    /** The "hide augments" button: says an offer is up even with the cards put away. */
    button: { score: number; present: boolean };
    /** Empty outside an offer: the card area is only captured once something cheaper found a reason. */
    cards: CardFrame[];
    cardsOnScreen: boolean;
    lastOcr: { texts: string[]; scores: (number | null)[]; atGameTime: number; millis: number } | null;
    /** The raw stat anvil reading, when the cards on screen are shards. */
    anvil: AnvilRead | null;
    recentEvents: string[];
  };
  staticData: {
    loaded: boolean;
    patch: string | null;
    locale: string | null;
    poolSize: number;
    offline: boolean;
    error: string | null;
  };
  /** The downloaded statistics every champion's numbers come from (D-090). Nothing is shown until
   *  one is loaded. Mirrors `snapshot::DatasetView`. */
  dataset: {
    loaded: boolean;
    patch: string | null;
    dataDate: string | null;
    champions: number;
    /** Unix seconds. */
    downloadedAt: number | null;
    /** Unix seconds; null since the app started. */
    checkedAt: number | null;
    /** `[received, total]` bytes while a download runs. */
    downloading: [number, number | null] | null;
    /** Why the last update failed. With a dataset loaded, the app carries on with it. */
    error: string | null;
  };
  /** The stat anvil offer on screen, ranked for the champion. */
  anvil: AnvilView;
  diagnostics: {
    /** Parts of the backend that have stopped running. Empty in a healthy app; nothing restarts
     *  them, so the app needs restarting. */
    faults: string[];
    /** The log file, or null when it could not be opened. */
    logFile: string | null;
    /** How `tuning.json` was taken: null when there is none, "applied", or why it was ignored. */
    tuning: string | null;
  };
}

export type AnvilTier = "silver" | "gold" | "prismatic";

export interface AnvilView {
  onScreen: boolean;
  tier: AnvilTier | null;
  /** The champion's ranking group; null means no labels (there is no fallback group). */
  group: string | null;
  /** Left to right; empty unless `onScreen`. */
  cards: (AnvilCard | null)[];
  /** The worst rank in this tier's ranking (dense, so the number of places); null when none. */
  rankOf: number | null;
  /** Why no labels are drawn, when they are not. */
  status: string | null;
  shardsLoaded: number;
  rankingsSavedAt: number | null;
  error: string | null;
  /** How the enemy team deals its damage, as fractions summing to 1. Null until it is known. */
  enemyDamage: { physical: number; magic: number; trueDamage: number } | null;
  /** Which of Armor and Magic Resist goes first where the rankings tie them. */
  resistFirst: "armor" | "magicResist" | null;
}

export interface AnvilCard {
  id: string;
  name: string;
  /** Dense rank in the tier's whole pool, 1 best; null when unranked. */
  rank: number | null;
  best: boolean;
}

export interface AnvilRead {
  titleTexts: string[];
  valueTexts: [string, string][];
  matches: ({ kind: string; score: number } | null)[];
  decision: { tier: AnvilTier | null; shards: (string | null)[]; problem: string | null };
}

/** The settings the companion window edits: all of `config.json`, and nothing else is in it
 *  (`src-tauri/src/config.rs` `Settings`). */
export interface AppConfig {
  /** Typed in, or remembered from the first time the client was found. */
  leagueDir: string | null;
  /** Write item sets on champion lock-in. Deletes every existing item page. */
  manageItemSets: boolean;
  /** Off hides the overlay everywhere; nothing else stops. */
  overlayEnabled: boolean;
  /** Every block and panel shows its tier badge and nothing else. */
  simpleMode: boolean;
  /** Draw the ranked augment list top right during an offer. */
  showAugmentList: boolean;
  /** Accept the ready check one second after a game is found. */
  autoAccept: boolean;
  /** Closing the companion window hides it to the tray instead of quitting. */
  keepInTray: boolean;
  /** Launch at Windows sign-in, with the companion window out of the way. */
  startMinimized: boolean;
  /** Ask GitHub Releases for a newer version when the app starts. Nothing installs without a click. */
  checkUpdatesOnStartup: boolean;
  /** Look for newer statistics every 24 hours. They are checked at every start regardless. */
  autoUpdateStatistics: boolean;
}

/** Where the update check stands. Mirrors `updates::UpdateStatus` in the backend. */
export type UpdateStatus =
  | { state: "idle" }
  | { state: "checking" }
  | { state: "upToDate"; current: string }
  | { state: "available"; current: string; version: string; notes: string | null }
  | { state: "downloading"; version: string; downloaded: number; total: number | null }
  | { state: "installing"; version: string }
  | { state: "failed"; message: string };

/** How this copy runs. Mirrors `commands::InstallMode` in the backend. */
export interface InstallMode {
  /** Started from the release zip: updates come from the release page, not the installer. */
  portable: boolean;
  /** The `data` folder beside the executable, when settings are kept there. */
  dataDir: string | null;
}

/** The chat statuses the companion window can set. Mirrors `league_api::lcu::ChatStatus`. */
export type ChatStatus = "online" | "away" | "offline";

export interface CardFrame { score: number; present: boolean }

export interface Reroll { score: number; present: boolean; spent: boolean }
