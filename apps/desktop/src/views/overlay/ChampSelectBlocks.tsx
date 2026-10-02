import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";

import type { ChampSelectSlot, Snapshot } from "../../types";
import { gradeColour, TierBadge } from "./TierBadge";

/**
 * The statistics blocks on the ARAM: Mayhem champ-select screen.
 *
 * Three surfaces, all the same block: the pick cards in the middle (up for about fifteen seconds),
 * the available-champions strip along the top, and the ally column down the left. Positions come
 * from a layout fitted to real captures; identities from the LCU.
 *
 * The bench blocks double as the swap button, because the bench is the only thing a swap can target
 * and, unlike the cards, it does not vanish part-way through being aimed at.
 */
export function ChampSelectBlocks({ s }: { s: Snapshot }) {
  return (
    <>
      {s.champSelect.slots.map((slot) => (
        // Keyed by who is in the slot as well as where it is. Keyed by position alone, a block kept
        // its state when the bench was rerolled, and a failed swap stayed marked on whichever
        // champion took that place next.
        <StatBlock
          key={`${slot.surface}-${slot.index}-${slot.championId}`}
          slot={slot}
          simple={s.options.simpleMode}
        />
      ))}
    </>
  );
}

/** Below this many games a champion's rates move on noise, so the block is marked.
 *
 *  `[unverified]` as a threshold: every champion in the table observed so far has at least 200,000
 *  games, so this is a guard against a champion released mid-patch rather than a line tuned on real
 *  thin rows. */
const LOW_SAMPLE = 10_000;

/**
 * One block: tier, rank, win rate, pick rate.
 *
 * Laid out the same on every surface so the eye learns one shape, and sized in `vh` so it scales
 * with the client window rather than with the desktop.
 */
function StatBlock({ slot, simple }: { slot: ChampSelectSlot; simple: boolean }) {
  const [pending, setPending] = useState(false);
  const [failed, setFailed] = useState(false);

  // Percentages of the overlay, which covers the client window's client area exactly, so no DPI
  // arithmetic is needed and it survives the client being resized.
  //
  // In simple mode the wide blocks - the pick cards and the ally rows - shrink to their badge, centred
  // on where the full block would be. The strip keeps its size: it is an icon wide already, and its
  // blocks are the swap buttons, whose clickable area the backend tracks at full size.
  const shrunk = simple && slot.surface !== "strip" && !slot.swappable;
  const style: React.CSSProperties = shrunk
    ? {
        left: `${(slot.x + slot.width / 2) * 100}%`,
        top: `${slot.y * 100}%`,
        height: `${slot.height * 100}%`,
        transform: "translateX(-50%)",
      }
    : {
        left: `${slot.x * 100}%`,
        top: `${slot.y * 100}%`,
        width: `${slot.width * 100}%`,
        height: `${slot.height * 100}%`,
      };

  const swap = () => {
    if (!slot.swappable || pending) return;
    setPending(true);
    setFailed(false);
    invoke("swap_to_champion", { championId: slot.championId })
      .catch(() => setFailed(true))
      .finally(() => setPending(false));
  };

  const stats = slot.stats;
  // The strip slots are an icon wide, so they get the stacked shape with the tier symbol astride the
  // top edge. The cards and the ally rows are wide and short, so they get the row.
  const compact = slot.surface === "strip";

  // No row in the champion table: a champion released this patch, or the table has not arrived. The
  // block stays — on the bench it is the swap button — and shows dashes. Nothing here invents a
  // number to fill the space, because a plausible-looking win rate is indistinguishable from a real
  // one and would be trusted as such.
  const tier = stats?.tier ?? "?";
  const badge = <TierBadge tier={tier} className="ov-cs-symbol" solid />;

  let body: React.ReactNode;
  if (simple) {
    // Simple mode: the badge alone, centred.
    body = badge;
  } else if (!stats) {
    body = (
      <>
        {badge}
        <span className="ov-cs-win">--</span>
        <span className="ov-cs-place ov-cs-of">no data</span>
      </>
    );
  } else {
    const { rank, poolSize, winRate, pickRate } = stats;
    // The same rule as the augment panels: the number carrying the judgement is coloured, the context
    // around it stays muted. A win rate is a judgement against even odds, so it is green above and red
    // below, with a half-point dead band either side of 50 — champion win rates cluster there, and a
    // figure that flips colour on noise is worse than one that does not colour at all.
    const win = (
      <span className={`ov-cs-win${winRate > 50.5 ? " ov-delta-up" : winRate < 49.5 ? " ov-delta-down" : ""}`}>
        {Math.round(winRate)}%
      </span>
    );
    // The rank is meaningless without what it is out of, so the two never separate — and the rank takes
    // the tier's colour, because the tier is what the rank means.
    const place = (
      <span className="ov-cs-place">
        <span style={{ color: gradeColour(tier) }}># {rank}</span> <span className="ov-cs-of">/ {poolSize}</span>
      </span>
    );
    body = compact ? (
      <>
        {badge}
        {win}
        {place}
        <span className="ov-cs-pick">{Math.round(pickRate)}%</span>
      </>
    ) : (
      <>
        <span className="ov-cs-figures">
          {win}
          {place}
        </span>
        {badge}
        <span className="ov-cs-pick">
          Pick Rate :<br />
          {Math.round(pickRate)}%
        </span>
      </>
    );
  }

  const className = [
    "ov-cs-block",
    compact ? "ov-cs-stack" : "ov-cs-row",
    `ov-cs-${slot.surface}`,
    stats ? "" : "ov-cs-nodata",
    // Fewer games than this and the rates move on noise, so the block is marked rather than trusted.
    stats && stats.sampleCount > 0 && stats.sampleCount < LOW_SAMPLE ? "ov-cs-thin" : "",
    failed ? "ov-cs-failed" : "",
    simple ? "ov-cs-simple" : "",
  ]
    .filter(Boolean)
    .join(" ");

  // Only the bench is a button. Everything else is a plain box, so it cannot swallow a click meant
  // for the client underneath.
  return slot.swappable ? (
    <button className={className} style={style} onClick={swap} disabled={pending} title="Swap to this champion">
      {body}
    </button>
  ) : (
    <div className={className} style={style}>
      {body}
    </div>
  );
}
