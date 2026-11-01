import { decimal, currentLocale, detail, type Language, t } from "../i18n";
import { useState } from "react";
import { useConfig } from "../useConfig";
import { rarity } from "../format";
import type { Snapshot } from "../types";

/** The advanced screen behind the gear: where League is, what data is loaded, and what the screen
 *  reader is seeing. */
export function Settings({ s, onBack }: { s: Snapshot; onBack: () => void }) {
  const v = s.vision;
  const score = (m: { score: number; present: boolean }) => `${decimal(m.score, 3)}${m.present ? " ✓" : ""}`;
  // The reroll scan stops at the first button it finds, so the ones after a hit were not looked at.
  const firstReroll = v.rerolls.findIndex((r) => r.present);

  const pct = (share: number) => `${decimal(share * 100, 1)}%`;

  const cs = s.champSelect;
  // A champion the table has no row for, on a screen that otherwise has data.
  const unranked = cs.rankedChampions > 0 ? cs.slots.filter((slot) => slot.stats === null).length : 0;
  // Everything for the locked champion is downloaded before the game, so no offer waits on the network.
  const prep = s.champion;
  const prepText = prep.error
    ? t("incomplete · {detail}", { detail: detail(prep.error) })
    : prep.ready
      ? t("ready")
      : prep.loading || prep.championId !== null
        ? t("preparing · {loaded}/{expected} pools{build}", { loaded: prep.poolsLoaded, expected: prep.poolsExpected, build: prep.buildLoaded ? t(" · build") : "" })
        : t("no champion locked");

  return (
    <div className="page companion">
      <header>
        <h1>{t("Settings")}</h1>
        <button onClick={onBack}>{t("Back")}</button>
      </header>

      <LanguagePicker />
      <LeagueDir detected={s.client.installDir} />

      {/* What the overlay's own status chips used to say, before they were taken off the game and
          the client. This is now the only place any of it shows. */}
      <section>
        <h2>{t("Status")}</h2>
        <dl>
          <dt>{t("Champion table")}</dt>
          <dd className={cs.rankedChampions === 0 && cs.rankingsError ? "error" : ""}>
            {cs.rankedChampions > 0
              ? t("{count} champions · patch {patch} ({date})", { count: cs.rankedChampions, patch: cs.dataPatch ?? "—", date: cs.dataDate ?? "—" })
              : ((cs.rankingsError ? detail(cs.rankingsError) : null) ?? t("not loaded yet"))}
            {cs.freshness === "stale" && <span className="pill">{t("offline copy")}</span>}
          </dd>
          <dt>{t("Champ select")}</dt>
          <dd>
            {cs.active
              ? t("{phase} · {offered} offered · {bench} on the bench · {blocks} blocks", { phase: detail(cs.phase.toLowerCase().replace("_", " ")), offered: cs.cardCount, bench: cs.benchSize, blocks: cs.slots.length })
              : t("not open")}
            {unranked > 0 && t(" · {count} not ranked", { count: unranked })}
            {cs.stale && <div className="warn">{t("Client not answering, showing the last read")}</div>}
          </dd>
          <dt>{t("Last swap")}</dt>
          <dd>{(cs.lastSwap ? detail(cs.lastSwap) : null) ?? "—"}</dd>
          <dt>{t("Champion data")}</dt>
          <dd className={prep.error ? "error" : ""}>{prepText}</dd>
        </dl>
      </section>

      <section>
        <h2>{t("Screen reading")}</h2>
        <dl>
          <dt>{t("Available")}</dt>
          <dd>{v.available ? t("yes") : ((v.unavailableReason ? detail(v.unavailableReason) : null) ?? t("starting…"))}</dd>
          <dt>{t("Game window")}</dt>
          <dd>{v.gameWindowFound ? t("found {size}", { size: v.clientSize?.join("×") ?? "" }) : t("not found")}</dd>
          <dt>{t("Rate")}</dt>
          <dd>{decimal(v.samplesPerSecond, 1)} {t("samples/s")}</dd>
          <dt>{t("OCR engine")}</dt>
          <dd>{v.ocrEngine ?? t("not started")}</dd>
          <dt>{t("Last error")}</dt>
          <dd className={v.lastError ? "error" : ""}>{(v.lastError ? detail(v.lastError) : null) ?? "—"}</dd>
        </dl>
      </section>

      <section>
        <h2>{t("Augment OCR")}</h2>
        <p>
          {v.cardsOnScreen ? t("Cards on screen") : t("No cards")} · {(s.augments.ocrBlocker ? detail(s.augments.ocrBlocker) : null) ?? t("ready")}
        </p>
        {/* The gates in the order the pipeline asks them, with their match scores, so a failure can be
            placed: the rerolls say the cards are drawn, the hide button says an offer is up with the
            cards put away, and the card frames are only looked for once one of those found something. */}
        <dl>
          <dt>{t("Rerolls")}</dt>
          <dd>{v.rerolls.map((r, i) => (firstReroll >= 0 && i > firstReroll ? "—" : score(r))).join(" · ") || "—"}</dd>
          <dt>{t("Hide button")}</dt>
          <dd>{score(v.button)}</dd>
          <dt>{t("Card frames")}</dt>
          <dd>{v.cards.map(score).join(" · ") || "—"}</dd>
        </dl>
        {v.lastOcr ? (
          <>
            <table>
              <thead>
                <tr>
                  <th>{t("Card")}</th>
                  <th>{t("OCR text")}</th>
                  <th>{t("Match")}</th>
                </tr>
              </thead>
              <tbody>
                {v.lastOcr.texts.map((t, i) => (
                  <tr key={i}>
                    <td>{i + 1}</td>
                    <td>
                      <code>{t || "∅"}</code>
                    </td>
                    <td>{v.lastOcr!.scores[i] != null ? decimal(v.lastOcr!.scores[i]!, 2) : "—"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <p className="muted">{v.lastOcr.millis} {t("ms for three cards")}</p>
          </>
        ) : (
          <p className="muted">{t("No OCR pass yet.")}</p>
        )}

        <h3>{t("Stat anvil")}</h3>
        <p className="muted">
          {s.anvil.shardsLoaded} {t("shards loaded · rankings")}{" "}
          {s.anvil.rankingsSavedAt ? t("saved {date}", { date: new Date(s.anvil.rankingsSavedAt * 1000).toLocaleString(currentLocale()) }) : t("not loaded")}
          {s.anvil.error && ` · ${detail(s.anvil.error)}`}
        </p>
        {/* What breaks a tie between Armor and Magic Resist. Only known in game. */}
        <p className="muted">
          {t("Enemy damage:")}{" "}
          {s.anvil.enemyDamage
            ? t("{physical} physical · {magic} magic · {true} true, so {resist} goes first in a tie", { physical: pct(s.anvil.enemyDamage.physical), magic: pct(s.anvil.enemyDamage.magic), true: pct(s.anvil.enemyDamage.trueDamage), resist: s.anvil.resistFirst === "magicResist" ? t("Magic Resist") : t("Armor") })
            : t("not known, so an Armor / Magic Resist tie stays a tie")}
        </p>
        {v.anvil ? (
          <>
            <table>
              <thead>
                <tr>
                  <th>{t("Card")}</th>
                  <th>{t("Stat")}</th>
                  <th>{t("Shard")}</th>
                  <th>{t("Rank")}</th>
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
              {t("tier")} {v.anvil.decision.tier ? rarity(v.anvil.decision.tier).label : t("undecided")}
              {s.anvil.group && t(' · group "{group}"', { group: detail(s.anvil.group) })}
              {s.anvil.status && ` · ${detail(s.anvil.status)}`}
            </p>
          </>
        ) : (
          <p className="muted">{t("No anvil offer on screen.")}</p>
        )}
      </section>

      <section>
        <h2>{t("Recent events")}</h2>
        <ul className="events">
          {v.recentEvents.map((e, i) => (
            <li key={i}>
              <code>{detail(e)}</code>
            </li>
          ))}
        </ul>
      </section>

      {/* What to send with a bug report. The events above are the last few; the file has all of them. */}
      <section>
        <h2>{t("Diagnostics")}</h2>
        <dl>
          <dt>{t("Stopped")}</dt>
          <dd className={s.diagnostics.faults.length > 0 ? "error" : ""}>
            {s.diagnostics.faults.length > 0 ? s.diagnostics.faults.map((fault) => detail(fault)).join(" · ") : t("nothing")}
          </dd>
          <dt>{t("Log file")}</dt>
          <dd>{s.diagnostics.logFile ? <div className="path">{s.diagnostics.logFile}</div> : t("could not be opened")}</dd>
          {s.diagnostics.tuning && (
            <>
              <dt>tuning.json</dt>
              <dd className={s.diagnostics.tuning === "applied" ? "" : "error"}>{detail(s.diagnostics.tuning)}</dd>
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
function LanguagePicker() {
  const [cfg, save] = useConfig();
  const [error, setError] = useState<string | null>(null);
  return <section>
    <label className="field">
      {t("Language")}
      <select value={cfg?.language ?? "auto"} disabled={!cfg} onChange={(event) => {
        setError(null);
        save({ language: event.target.value as Language }).catch((reason) => setError(String(reason)));
      }}>
        <option value="auto">{t("Automatic (system)")}</option>
        <option value="en">English</option>
        <option value="pt-BR">Português (Brasil)</option>
      </select>
    </label>
    <p className="muted">{t("Interface language. Game names and OCR follow the League client.")}</p>
    {error && <p className="error">{detail(error)}</p>}
  </section>;
}

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
      .then(() => setMsg(t("Saved.")))
      .catch((e) => setMsg(String(e)));

  return (
    <section>
      <h2>League of Legends</h2>
      <label className="field">
        {t("Install directory")}
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
          {t("Save")}
        </button>
        <span className="muted">{(msg ? detail(msg) : "") || t("Found on its own while the client is running.")}</span>
      </div>
    </section>
  );
}
