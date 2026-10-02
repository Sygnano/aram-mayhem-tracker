import { rarity } from "../../format";
import type { AnvilTier, CardAnchor, RankedAugment, Snapshot } from "../../types";
import { gradeColour, TierBadge } from "./TierBadge";

// Nothing in this file ranks, grades or classifies an augment. Each row arrives with its block, its
// place in that block and its grade already decided (`engine/stats.rs`, `mayhem_core::ranking`);
// what is left here is how to draw them.

/** Signed, with a percent sign: the sign is the whole point. */
function delta(pp: number | null): string {
  if (pp === null) return "—";
  return `${pp >= 0 ? "+" : ""}${pp.toFixed(1)}%`;
}

/** The same, with the sign held off the number, which reads better in the per-level column. */
function spacedDelta(pp: number): string {
  return `${pp >= 0 ? "+" : "-"} ${Math.abs(pp).toFixed(1)}%`;
}

/**
 * The four offers, by aramkit stage number and the in-game level it happens at.
 *
 * Rendered in full every time. An augment that is simply not offered at a level - quests and the
 * like are not - shows a dash there, rather than the row disappearing and making the panel a
 * different height from its neighbours.
 */
const STAGES = [
  { stage: 1, level: 1 },
  { stage: 2, level: 7 },
  { stage: 3, level: 11 },
  { stage: 4, level: 15 },
] as const;

function percent(rate: number | null): string {
  return rate === null ? "—" : `${(rate * 100).toFixed(1)}%`;
}

/** 320503 → "320k", so a sample count fits on one line. */
function count(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${Math.round(n / 1_000)}k`;
  return String(n);
}

/**
 * Why a number should be read with caution, or null when it should not.
 *
 * Two different warnings, deliberately kept apart: `lowSample` means too few games behind an
 * otherwise champion-specific number, while the `global` block means the number is not about this
 * champion at all and its delta is measured against a different baseline.
 */
function caveat(a: RankedAugment): string | null {
  if (a.block === "none") return "no data";
  if (a.block === "global") return "all champions";
  if (a.lowSample) return `${count(a.sampleCount)} games`;
  return null;
}

/**
 * One panel per card, anchored by its bottom edge just above the reroll button.
 *
 * The anchors come from the backend, which owns the measured card layout; nothing here recomputes
 * where the cards are.
 */
export function CardPanels({ s }: { s: Snapshot }) {
  const offer = s.augments.offer;
  const anchors = s.augments.cardAnchors;

  if (!offer) {
    // Cards are on screen but not read yet: one quiet line rather than three empty panels.
    if (s.vision.cardsOnScreen) {
      return (
        <div className="ov-due">
          Reading augment cards…
          {s.augments.ocrBlocker && <span className="ov-due-why"> — {s.augments.ocrBlocker}</span>}
        </div>
      );
    }
    return null;
  }
  // Without the window size we cannot place anything; drawing panels in the wrong place over the
  // game would be worse than drawing none.
  if (!anchors) return null;

  const stats = s.stats.offer;
  const best = stats?.ranking[0];

  // A rerolled card is rendered from data we already hold rather than from a fresh request.
  //
  // The pool holds every augment of this rarity for this champion, and an ordinary reroll never
  // changes rarity, so the new card's numbers are already in memory the instant OCR names it. The
  // offer still wins when it has the card, since it is the answer for this exact offer.
  //
  // A golden reroll is the exception: its card is one rarity up, so its row comes with the
  // offer, carrying its place in that rarity's own list.
  const byId = new Map((s.stats.pool?.augments ?? []).map((x) => [x.id, x]));
  for (const a of stats?.augments ?? []) byId.set(a.id, a);
  const simple = s.options.simpleMode;

  return (
    <>
      {offer.cards.map((card, i) => {
        const anchor = anchors[i];
        if (!anchor) return null;
        const info = card ? byId.get(card.id) : undefined;
        const isBest = card != null && card.id === best && info?.block !== "none";

        return (
          <div
            key={i}
            className={`ov-panel${isBest ? " ov-panel-best" : ""}${simple ? " ov-panel-simple" : ""}`}
            style={panelStyle(anchor, card ? rarity(card.rarity).color : "#6a7382", simple)}
          >
            {card == null ? (
              <div className="ov-panel-empty">not read</div>
            ) : info ? (
              <CardStats info={info} stage={offer.stage} simple={simple} />
            ) : (
              <Pending s={s} />
            )}
          </div>
        );
      })}
    </>
  );
}

const ANVIL_TIER_COLOUR: Record<AnvilTier, string> = {
  silver: "#b9c2c7",
  gold: "#e2b94a",
  prismatic: "#d39cf2",
};

/**
 * A rank label on each stat anvil card, inside the card just above its bottom frame.
 *
 * The label reads `#n / nmax` so it is plainly a ranking. n is the shard's rank in the whole pool
 * for its tier, from the rankings authored in the service's editor: the same shard always shows the
 * same number for the same champion, and ties share one. nmax is the worst rank in that ranking. The best card on screen is highlighted. An unranked shard gets no label, and when
 * nothing can be labelled - no group for this champion, or the tier is not settled - one quiet line
 * says why instead of three empty boxes.
 */
export function AnvilLabels({ s }: { s: Snapshot }) {
  const anvil = s.anvil;
  const anchors = s.augments.cardAnchors;
  const labelled = anvil.cards.some((c) => c?.rank != null);

  if (!labelled || !anchors) {
    const why = anvil.status ?? (anvil.group ? "no ranked shard in this offer" : null);
    return why ? <div className="ov-due">Stat anvil — <span className="ov-due-why">{why}</span></div> : null;
  }
  const colour = anvil.tier ? ANVIL_TIER_COLOUR[anvil.tier] : "#6a7382";

  return (
    <>
      {anvil.cards.map((card, i) => {
        const anchor = anchors[i];
        if (!anchor || !card || card.rank == null) return null;
        return (
          <div
            key={i}
            className={`ov-anvil${card.best ? " ov-anvil-best" : ""}`}
            style={{ left: `${anchor.centerX * 100}%`, top: `${anchor.anvilBottomY * 100}%`, borderColor: colour }}
            title={`${card.name} · ${anvil.group}`}
          >
            <AnvilMark color={colour} />
            <span className="ov-anvil-rank">#{card.rank}</span>
            {anvil.rankOf !== null && <span className="ov-anvil-rank-of">/ {anvil.rankOf}</span>}
          </div>
        );
      })}
    </>
  );
}

/** A small anvil silhouette, so the label is never mistaken for an aramkit tier badge. */
function AnvilMark({ color }: { color: string }) {
  return (
    <svg className="ov-anvil-mark" viewBox="0 0 24 16" aria-label="Stat anvil rank">
      <path d="M2 2 H17 C19 2 22 3 23 5 C20 5 18 6 17 7 V8 C17 9 16 10 14 10 V12 H17 V15 H5 V12 H8 V10 C6 10 5 9 5 8 V6 C3 6 2 4 2 2 Z" fill={color} />
    </svg>
  );
}

/**
 * Places a panel: horizontally centred on the card, bottom edge on the anchor line. The width comes
 * from the backend (half the card frame by default). `translate(-50%, -100%)` makes it grow upward.
 * In simple mode there is no width: the panel is as wide as its badge.
 */
function panelStyle(anchor: CardAnchor, borderColor: string, simple: boolean): React.CSSProperties {
  return {
    left: `${anchor.centerX * 100}%`,
    top: `${anchor.panelBottomY * 100}%`,
    width: simple ? undefined : `${anchor.width * 100}%`,
    borderColor,
  };
}

function CardStats({ info, stage, simple }: { info: RankedAugment; stage: number; simple: boolean }) {
  const why = caveat(info);
  const sign = info.deltaPp === null ? "" : info.deltaPp >= 0 ? " ov-delta-up" : " ov-delta-down";

  if (info.block === "none") {
    return <div className="ov-panel-empty">no data</div>;
  }
  if (simple) return <TierBadge tier={info.grade} />;
  return (
    <div className="ov-panel-row">
      <TierBadge tier={info.grade} />

      <div className="ov-col ov-col-delta">
        <div className={`ov-delta${sign}${why ? " ov-delta-weak" : ""}`}>{delta(info.deltaPp)}</div>
        {/* The place within the row's own block, among the augments of its own rarity: an
            all-champion fallback is never numbered among champion-specific rows. */}
        {info.poolRank !== null && (
          <div className="ov-rank">
            <span style={{ color: gradeColour(info.grade) }}>#{info.poolRank}</span>
            <span className="ov-rank-of"> / {info.poolRankOf}</span>
          </div>
        )}
      </div>

      <div className="ov-sep" />

      {/* A two-column grid, so both values start at the same x rather than trailing their labels. */}
      <div className="ov-col ov-rates">
        <span className="ov-rate-label">WR</span>
        <span className="ov-rate-value">{percent(info.winRate)}</span>
        <span className="ov-rate-label">PR</span>
        <span className="ov-rate-value">{percent(info.pickRate)}</span>
      </div>

      <div className="ov-sep" />

      {/* The four offers, at levels 1, 7, 11 and 15. Only the one being offered now reads at full
          strength; the others are dimmed because they are not the decision in front of you. */}
      <div className="ov-col ov-levels">
        {STAGES.map(({ stage: n, level }) => {
          const row = info.byStage.find((b) => b.stage === n);
          return (
            <div
              key={n}
              className={`ov-level${n === stage ? " ov-level-now" : ""}${
                row?.lowSample ? " ov-level-weak" : ""
              }`}
            >
              <span className="ov-level-n">{level}</span>
              <span className="ov-level-dot">•</span>
              <span className="ov-level-v">
                {row ? spacedDelta(row.deltaPp) : <span className="ov-level-absent">—</span>}
              </span>
            </div>
          );
        })}
      </div>

      {why && <div className="ov-panel-caveat">{why}</div>}
    </div>
  );
}

function Pending({ s }: { s: Snapshot }) {
  if (s.stats.championUnknown) return <div className="ov-panel-empty">champion unknown</div>;
  if (s.stats.error) return <div className="ov-panel-empty">stats unavailable</div>;
  return <div className="ov-panel-empty">…</div>;
}

/**
 * The ranked list for the offer's rarity, top right.
 *
 * The cards in an offer share one rarity, ordinary rerolls included, so the offer on screen decides
 * which of the three lists is open. A golden reroll puts one card a rarity higher; the list
 * stays on the offer's own rarity and that card is not in it, since its rank belongs to another
 * list and is shown on its panel.
 *
 * **Only augments aramkit has data for on this champion are listed.** The rest are not missing data,
 * they are augments this champion cannot be offered: for Yasuo, an AD manaless champion, the block
 * with no champion data is AP and mana augments. Listing them alongside real numbers made the list
 * run S to D twice and invited comparisons between deltas measured against different baselines
 *.
 *
 * If a card on screen somehow is not in that list, it is appended with a `?` tier and `??` delta
 * rather than silently dropped. That should not be reachable; it exists so a surprise is visible.
 *
 * Two states are called out. An augment that cannot appear at this stage is dimmed, and one already
 * on screen this game is struck through: rerolled away, passed over, or taken, it will not come back
 * either way.
 */
export function RarityList({ s }: { s: Snapshot }) {
  const pool = s.stats.pool;
  const offer = s.augments.offer;
  if (!pool || !offer) return null;

  const offered = new Set(offer.cards.flatMap((c) => (c ? [c.id] : [])));
  const seen = new Set(s.augments.seenAugmentIds);

  const ranked = pool.augments.filter((a) => a.block === "champion");
  const listed = new Set(ranked.map((a) => a.id));

  type Row = {
    id: number;
    name: string;
    /** Null only for the should-not-happen appended rows. */
    rank: number | null;
    grade: string;
    colour: string;
    deltaText: string;
    positive: boolean;
    unavailable: boolean;
    used: boolean;
    offered: boolean;
  };

  const rows: Row[] = ranked.map((a) => ({
    id: a.id,
    name: a.name ?? `#${a.id}`,
    rank: a.poolRank,
    grade: a.grade,
    colour: gradeColour(a.grade),
    deltaText: delta(a.deltaPp),
    positive: a.deltaPp !== null && a.deltaPp >= 0,
    // An empty availableStages means upstream did not say, so assume it can appear.
    unavailable: a.availableStages.length > 0 && !a.availableStages.includes(offer.stage),
    // The cards in front of you are struck through only once they are gone, not while offered.
    used: seen.has(a.id) && !offered.has(a.id),
    offered: offered.has(a.id),
  }));

  // The should-not-happen case: a card on screen that the champion's own list does not contain. A
  // golden reroll is not that case: it is simply in the list one rarity up.
  const upgraded = new Set((s.stats.upgradedPool?.augments ?? []).map((a) => a.id));
  for (const card of offer.cards) {
    if (!card || listed.has(card.id) || upgraded.has(card.id)) continue;
    rows.push({
      id: card.id,
      name: card.name,
      rank: null,
      grade: "?",
      colour: "#9aa4b2",
      deltaText: "??",
      positive: true,
      unavailable: false,
      used: false,
      offered: true,
    });
  }

  return (
    <div className="ov-pool" style={{ borderColor: rarity(pool.rarity).color }}>
      {/* No heading: the rarity is obvious from the cards, and the rows speak for themselves. The
          one thing worth saying is that the numbers are not fresh. */}
      {s.stats.freshness === "stale" && <div className="ov-pool-stale">offline copy</div>}
      <ol className="ov-pool-list">
        {rows.map((r) => {
          const classes = ["ov-pool-row"];
          if (r.offered) classes.push("ov-pool-offered");
          if (r.unavailable) classes.push("ov-pool-unavailable");
          if (r.used) classes.push("ov-pool-used");

          return (
            <li key={r.id} className={classes.join(" ")}>
              {/* A plain letter here: the hexagon badge does not read at this size. */}
              <span className="ov-pool-tier" style={{ color: r.colour }}>
                {r.grade}
              </span>
              <span className="ov-pool-rank">{r.rank === null ? "?" : `#${r.rank}`}</span>
              <span className="ov-pool-name">{r.name}</span>
              <span className={r.positive ? "ov-delta-up" : "ov-delta-down"}>{r.deltaText}</span>
            </li>
          );
        })}
      </ol>
    </div>
  );
}
