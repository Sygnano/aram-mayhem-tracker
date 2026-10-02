import { useState } from "react";
import { useConfig } from "../useConfig";
import type { Snapshot } from "../types";

/** The advanced screen behind the gear: where League is, what data is loaded, and what the screen
 *  reader is seeing. */
export function Settings({ s, onBack }: { s: Snapshot; onBack: () => void }) {
  const v = s.vision;
  const score = (m: { score: number; present: boolean }) => `${m.score.toFixed(3)}${m.present ? " ✓" : ""}`;
  // The reroll scan stops at the first button it finds, so the ones after a hit were not looked at.
  const firstReroll = v.rerolls.findIndex((r) => r.present);

  const pct = (share: number) => `${(share * 100).toFixed(1)}%`;

  const cs = s.champSelect;
  // A champion the table has no row for, on a screen that otherwise has data.
  const unranked = cs.rankedChampions > 0 ? cs.slots.filter((slot) => slot.stats === null).length : 0;
  // Everything for the locked champion is downloaded before the game, so no offer waits on the network.
  const prep = s.champion;
  const prepText = prep.error
    ? `incomplete · ${prep.error}`
    : prep.ready
      ? "ready"
      : prep.loading || prep.championId !== null
        ? `preparing · ${prep.poolsLoaded}/${prep.poolsExpected} pools${prep.buildLoaded ? " · build" : ""}`
        : "no champion locked";

  return (
    <div className="page companion">
      <header>
        <h1>Settings</h1>
        <button onClick={onBack}>Back</button>
      </header>

      <LeagueDir detected={s.client.installDir} />

      {/* What the overlay's own status chips used to say, before they were taken off the game and
          the client. This is now the only place any of it shows. */}
      <section>
        <h2>Status</h2>
        <dl>
          <dt>Champion table</dt>
          <dd className={cs.rankedChampions === 0 && cs.rankingsError ? "error" : ""}>
            {cs.rankedChampions > 0
              ? `${cs.rankedChampions} champions · patch ${cs.dataPatch} (${cs.dataDate})`
              : (cs.rankingsError ?? "not loaded yet")}
            {cs.freshness === "stale" && <span className="pill">offline copy</span>}
          </dd>
          <dt>Champ select</dt>
          <dd>
            {cs.active
              ? `${cs.phase.toLowerCase().replace("_", " ")} · ${cs.cardCount} offered · ${cs.benchSize} on the bench · ${cs.slots.length} blocks`
              : "not open"}
            {unranked > 0 && ` · ${unranked} not ranked`}
            {cs.stale && <div className="warn">Client not answering, showing the last read</div>}
          </dd>
          <dt>Last swap</dt>
          <dd>{cs.lastSwap ?? "—"}</dd>
          <dt>Champion data</dt>
          <dd className={prep.error ? "error" : ""}>{prepText}</dd>
        </dl>
      </section>

      <section>
        <h2>Screen reading</h2>
        <dl>
          <dt>Available</dt>
          <dd>{v.available ? "yes" : (v.unavailableReason ?? "starting…")}</dd>
          <dt>Game window</dt>
          <dd>{v.gameWindowFound ? `found ${v.clientSize?.join("×") ?? ""}` : "not found"}</dd>
          <dt>Rate</dt>
          <dd>{v.samplesPerSecond.toFixed(1)} samples/s</dd>
          <dt>OCR engine</dt>
          <dd>{v.ocrEngine ?? "not started"}</dd>
          <dt>Last error</dt>
          <dd className={v.lastError ? "error" : ""}>{v.lastError ?? "—"}</dd>
        </dl>
      </section>

      <section>
        <h2>Augment OCR</h2>
        <p>
          {v.cardsOnScreen ? "Cards on screen" : "No cards"} · {s.augments.ocrBlocker ?? "ready"}
        </p>
        {/* The gates in the order the pipeline asks them, with their match scores, so a failure can be
            placed: the rerolls say the cards are drawn, the hide button says an offer is up with the
            cards put away, and the card frames are only looked for once one of those found something. */}
        <dl>
          <dt>Rerolls</dt>
          <dd>{v.rerolls.map((r, i) => (firstReroll >= 0 && i > firstReroll ? "—" : score(r))).join(" · ") || "—"}</dd>
          <dt>Hide button</dt>
          <dd>{score(v.button)}</dd>
          <dt>Card frames</dt>
          <dd>{v.cards.map(score).join(" · ") || "—"}</dd>
        </dl>
        {v.lastOcr ? (
          <>
            <table>
              <thead>
                <tr>
                  <th>Card</th>
                  <th>OCR text</th>
                  <th>Match</th>
                </tr>
              </thead>
              <tbody>
                {v.lastOcr.texts.map((t, i) => (
                  <tr key={i}>
                    <td>{i + 1}</td>
                    <td>
                      <code>{t || "∅"}</code>
                    </td>
                    <td>{v.lastOcr!.scores[i]?.toFixed(2) ?? "—"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <p className="muted">{v.lastOcr.millis} ms for three cards</p>
          </>
        ) : (
          <p className="muted">No OCR pass yet.</p>
        )}

        <h3>Stat anvil</h3>
        <p className="muted">
          {s.anvil.shardsLoaded} shards loaded · rankings{" "}
          {s.anvil.rankingsSavedAt ? `saved ${new Date(s.anvil.rankingsSavedAt * 1000).toLocaleString()}` : "not loaded"}
          {s.anvil.error && ` · ${s.anvil.error}`}
        </p>
        {/* What breaks a tie between Armor and Magic Resist. Only known in game. */}
        <p className="muted">
          Enemy damage:{" "}
          {s.anvil.enemyDamage
            ? `${pct(s.anvil.enemyDamage.physical)} physical · ${pct(s.anvil.enemyDamage.magic)} magic · ${pct(s.anvil.enemyDamage.trueDamage)} true, so ${
                s.anvil.resistFirst === "magicResist" ? "Magic Resist" : "Armor"
              } goes first in a tie`
            : "not known, so an Armor / Magic Resist tie stays a tie"}
        </p>
        {v.anvil ? (
          <>
            <table>
              <thead>
                <tr>
                  <th>Card</th>
                  <th>Stat</th>
                  <th>Shard</th>
                  <th>Rank</th>
                </tr>
              </thead>
              <tbody>
                {v.anvil.titleTexts.map((_, i) => (
                  <tr key={i}>
                    <td>{i + 1}</td>
                    <td>{v.anvil!.matches[i]?.kind ?? "∅"}</td>
                    <td>{v.anvil!.decision.shards[i] ?? "∅"}</td>
                    <td>{s.anvil.cards[i]?.rank ?? "–"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <p className="muted">
              tier {v.anvil.decision.tier ?? "undecided"}
              {s.anvil.group && ` · group "${s.anvil.group}"`}
              {s.anvil.status && ` · ${s.anvil.status}`}
            </p>
          </>
        ) : (
          <p className="muted">No anvil offer on screen.</p>
        )}
      </section>

      <section>
        <h2>Recent events</h2>
        <ul className="events">
          {v.recentEvents.map((e, i) => (
            <li key={i}>
              <code>{e}</code>
            </li>
          ))}
        </ul>
      </section>

      {/* What to send with a bug report. The events above are the last few; the file has all of them. */}
      <section>
        <h2>Diagnostics</h2>
        <dl>
          <dt>Stopped</dt>
          <dd className={s.diagnostics.faults.length > 0 ? "error" : ""}>
            {s.diagnostics.faults.length > 0 ? s.diagnostics.faults.join(" · ") : "nothing"}
          </dd>
          <dt>Log file</dt>
          <dd>{s.diagnostics.logFile ? <div className="path">{s.diagnostics.logFile}</div> : "could not be opened"}</dd>
          {s.diagnostics.tuning && (
            <>
              <dt>tuning.json</dt>
              <dd className={s.diagnostics.tuning === "applied" ? "" : "error"}>{s.diagnostics.tuning}</dd>
            </>
          )}
        </dl>
      </section>
    </div>
  );
}

/**
 * The install directory. Filled with the saved one, or failing that with wherever the running
 * client was found, so it only needs typing when neither is known.
 */
function LeagueDir({ detected }: { detected: string | null }) {
  const [cfg, update] = useConfig();
  // What was typed, or null while nothing has been.
  const [typed, setTyped] = useState<string | null>(null);
  const [msg, setMsg] = useState("");
  // Until something is typed the field shows what is known, which can arrive after the screen
  // opens: the client may be found a few seconds in. Derived on each render, not copied into state.
  const shown = typed ?? cfg?.leagueDir ?? detected ?? "";

  const save = () =>
    update({ leagueDir: shown.trim() || null })
      .then(() => setMsg("Saved."))
      .catch((e) => setMsg(String(e)));

  return (
    <section>
      <h2>League of Legends</h2>
      <label className="field">
        Install directory
        <input
          value={shown}
          placeholder="C:\Riot Games\League of Legends"
          spellCheck={false}
          onChange={(e) => {
            setTyped(e.target.value);
            setMsg("");
          }}
        />
      </label>
      <div className="row" style={{ marginTop: 8 }}>
        <button onClick={save} disabled={!cfg}>
          Save
        </button>
        <span className="muted">{msg || "Found on its own while the client is running."}</span>
      </div>
    </section>
  );
}
