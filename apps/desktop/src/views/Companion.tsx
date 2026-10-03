import { useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useSnapshot } from "../useSnapshot";
import { useConfig } from "../useConfig";
import { progress, readiness } from "../readiness";
import type { ChatStatus, InstallMode, Snapshot, UpdateStatus } from "../types";
import { Settings } from "./Settings";

export function Companion() {
  const s = useSnapshot();
  const [settingsOpen, setSettingsOpen] = useState(false);
  if (!s) return <div className="page">Starting…</div>;
  return settingsOpen ? <Settings s={s} onBack={() => setSettingsOpen(false)} /> : <Home s={s} onSettings={() => setSettingsOpen(true)} />;
}

function Home({ s, onSettings }: { s: Snapshot; onSettings: () => void }) {
  const [cfg, save] = useConfig();
  // Why the last change was refused, if it was. A checkbox that silently snapped back, or stayed
  // ticked while nothing happened, used to be all there was to see: starting with Windows can fail,
  // for one.
  const [refused, setRefused] = useState<string | null>(null);
  const update = (patch: Parameters<typeof save>[0]) => {
    setRefused(null);
    save(patch).catch((e) => setRefused(String(e)));
  };
  const ready = readiness(s);
  const [mode, setMode] = useState<InstallMode | null>(null);
  useEffect(() => {
    invoke<InstallMode>("get_install_mode").then(setMode).catch(console.error);
  }, []);
  const portable = mode?.portable ?? false;

  return (
    <div className="page companion">
      <header>
        <h1>ARAM Mayhem Tracker</h1>
        <button className="icon-btn" onClick={onSettings} title="Settings" aria-label="Settings">
          <Gear />
        </button>
      </header>

      <section>
        <h2>Client</h2>
        <dl>
          <dt>Status</dt>
          <dd>
            <span className={`status-dot status-${ready.level}`} />
            {ready.text}
          </dd>
          <dt>Patch</dt>
          <dd>{s.staticData.patch ?? "—"}</dd>
          <dt>League of Legends</dt>
          <dd>
            {s.client.installDir ? (
              <>
                Detected
                <div className="path">{s.client.installDir}</div>
              </>
            ) : (
              "Not detected"
            )}
          </dd>
        </dl>
      </section>

      {refused && <p className="error">That setting was not saved: {refused}</p>}

      {cfg && (
        <>
          <section>
            <h2>Statistics</h2>
            <dl>
              <dt>Data</dt>
              <dd>
                {s.dataset.loaded
                  ? `${s.dataset.patch} · ${s.dataset.dataDate} · ${s.dataset.champions} champions`
                  : "Not downloaded yet"}
              </dd>
              {s.dataset.downloadedAt !== null && (
                <>
                  <dt>Downloaded</dt>
                  <dd>{when(s.dataset.downloadedAt)}</dd>
                </>
              )}
            </dl>
            <div className="checks">
              <Check
                label="Update statistics automatically"
                checked={cfg.autoUpdateStatistics}
                onChange={(on) => update({ autoUpdateStatistics: on })}
              >
                Looks for newer ones every 24 hours. They are always checked when the app starts.
              </Check>
              <div className="updates">
                <button
                  onClick={() => invoke<void>("update_statistics").catch(console.error)}
                  disabled={s.dataset.downloading !== null}
                >
                  Update now
                </button>
                <StatisticsLine s={s} />
              </div>
            </div>
          </section>

          <section>
            <h2>Overlay options</h2>
            <div className="checks">
              <Check label="Enabled" checked={cfg.overlayEnabled} onChange={(on) => update({ overlayEnabled: on })} />
              {/* Turning simple mode on takes the list down with it. The list keeps its own box, so
                  ticking that again brings it back while simple mode stays on. */}
              <Check
                label="Simple mode"
                checked={cfg.simpleMode}
                onChange={(on) => update(on ? { simpleMode: true, showAugmentList: false } : { simpleMode: false })}
              >
                Tier badges only, no numbers.
              </Check>
              <Check label="Show augment list" checked={cfg.showAugmentList} onChange={(on) => update({ showAugmentList: on })} />
              <Check label="Manage item sets" checked={cfg.manageItemSets} onChange={(on) => update({ manageItemSets: on })}>
                Replaces <strong>every item page on your account</strong> each time you lock in a champion.
              </Check>
            </div>
          </section>

          <section>
            <h2>Game</h2>
            <div className="checks">
              <Check label="Auto accept" checked={cfg.autoAccept} onChange={(on) => update({ autoAccept: on })}>
                Accepts one second after a game is found.
              </Check>
              <ChatStatusRadios connected={s.client.connected} />
            </div>
          </section>

          <section>
            <h2>Parameters</h2>
            <div className="checks">
              <Check label="Keep in tray" checked={cfg.keepInTray} onChange={(on) => update({ keepInTray: on })} />
              <Check label="Start with Windows minimized" checked={cfg.startMinimized} onChange={(on) => update({ startMinimized: on })}>
                {portable && "Starts this copy from where it is now. Untick it before moving the folder."}
              </Check>
            </div>
            {mode?.portable &&
              (mode.dataDir ? (
                <p className="muted">
                  Portable copy: settings and logs are kept in{" "}
                  <span className="path">{mode.dataDir}</span>
                </p>
              ) : (
                <p className="error">
                  Portable copy, but its folder cannot be written: settings are kept in AppData instead. Extract the
                  zip to a folder you own.
                </p>
              ))}
          </section>

          <section>
            <h2>Updates</h2>
            <div className="checks">
              <Check
                label="Check updates on startup"
                checked={cfg.checkUpdatesOnStartup}
                onChange={(on) => update({ checkUpdatesOnStartup: on })}
              />
              <UpdatePanel portable={portable} />
            </div>
          </section>
        </>
      )}
    </div>
  );
}

/** Where the last statistics update stands. Silent before the first check of this run. */
function StatisticsLine({ s }: { s: Snapshot }) {
  const d = s.dataset;
  if (d.downloading) return <p className="muted">Downloading… {progress(d.downloading)}</p>;
  if (d.error) {
    // With statistics on disk a failure is a note, not a fault: the app carries on with them.
    return <p className={d.loaded ? "muted" : "error"}>Update failed: {d.error}</p>;
  }
  if (d.checkedAt !== null) return <p className="muted">Up to date, checked {when(d.checkedAt)}.</p>;
  return null;
}

/** Unix seconds as the user's own date and time. */
function when(unix: number): string {
  return new Date(unix * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

function Check(props: { label: string; checked: boolean; onChange: (on: boolean) => void; children?: ReactNode }) {
  return (
    <label className="check">
      <input type="checkbox" checked={props.checked} onChange={(e) => props.onChange(e.target.checked)} />
      <span>
        {props.label}
        {props.children && <em>{props.children}</em>}
      </span>
    </label>
  );
}

/** States that end on their own, during which the status is asked for again. */
const UPDATE_BUSY: UpdateStatus["state"][] = ["checking", "downloading", "installing"];

/**
 * "Check for updates", what the check found, and "Download update" when there is something newer.
 * The status lives in the backend, so a check started at launch shows here too. A portable copy
 * cannot run the installer, so it gets the release page instead of the download.
 */
function UpdatePanel({ portable }: { portable: boolean }) {
  const [status, setStatus] = useState<UpdateStatus>({ state: "idle" });
  const busy = UPDATE_BUSY.includes(status.state);

  useEffect(() => {
    invoke<UpdateStatus>("get_update_status").then(setStatus).catch(console.error);
  }, []);
  // While something runs, follow it: a download reports its progress this way.
  useEffect(() => {
    if (!busy) return;
    const timer = setInterval(
      () => invoke<UpdateStatus>("get_update_status").then(setStatus).catch(console.error),
      300,
    );
    return () => clearInterval(timer);
  }, [busy]);

  const check = () => {
    setStatus({ state: "checking" });
    invoke<UpdateStatus>("check_for_updates")
      .then(setStatus)
      .catch((e) => setStatus({ state: "failed", message: String(e) }));
  };
  const openReleases = () => {
    invoke<void>("open_releases_page").catch((e) => setStatus({ state: "failed", message: String(e) }));
  };
  const download = () => {
    // The installer closes the app; this only resolves when it failed, and the status says why.
    invoke<void>("install_update").catch(() =>
      invoke<UpdateStatus>("get_update_status").then(setStatus).catch(console.error),
    );
    invoke<UpdateStatus>("get_update_status").then(setStatus).catch(console.error);
  };

  return (
    <div className="updates">
      <button onClick={check} disabled={busy}>
        Check for updates
      </button>
      <UpdateLine status={status} />
      {status.state === "available" &&
        (portable ? (
          <button onClick={openReleases} className="active">
            Open release page
          </button>
        ) : (
          <button onClick={download} className="active">
            Download update
          </button>
        ))}
    </div>
  );
}

function UpdateLine({ status }: { status: UpdateStatus }) {
  switch (status.state) {
    case "idle":
      return null;
    case "checking":
      return <p className="muted">Checking…</p>;
    case "upToDate":
      return <p className="muted">Already up to date ({status.current}).</p>;
    case "available":
      return (
        <p>
          Update available: <strong>{status.version}</strong> <span className="muted">(you have {status.current})</span>
        </p>
      );
    case "downloading": {
      const percent = status.total ? Math.floor((100 * status.downloaded) / status.total) : null;
      return <p className="muted">Downloading {status.version}…{percent !== null && ` ${percent}%`}</p>;
    }
    case "installing":
      return <p className="muted">Installing {status.version}. The app closes and restarts.</p>;
    case "failed":
      return <p className="error">{status.message}</p>;
  }
}

const CHAT_STATUSES: [ChatStatus, string][] = [
  ["online", "Online"],
  ["away", "Away"],
  ["offline", "Offline"],
];

/**
 * The chat status friends see. Picking one sends it to the client once. The button shown as chosen
 * is what the client reported when it was last asked, so none is chosen while the client holds a
 * status of its own (in queue, in game).
 */
function ChatStatusRadios({ connected }: { connected: boolean }) {
  const [status, setStatus] = useState<ChatStatus | null>(null);
  const refresh = () => invoke<ChatStatus | null>("get_chat_status").then(setStatus).catch(console.error);
  useEffect(() => {
    if (connected) refresh();
    else setStatus(null);
  }, [connected]);

  const pick = (next: ChatStatus) => {
    setStatus(next);
    // A refused change puts the buttons back on what the client really has.
    invoke<void>("set_chat_status", { status: next }).catch((e) => {
      console.error(e);
      refresh();
    });
  };

  return (
    <div className="radios" role="radiogroup" aria-labelledby="chat-status-label">
      <span id="chat-status-label">Set status</span>
      {CHAT_STATUSES.map(([value, label]) => (
        <label key={value}>
          <input type="radio" name="chat-status" disabled={!connected} checked={status === value} onChange={() => pick(value)} />
          {label}
        </label>
      ))}
    </div>
  );
}

function Gear() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h0a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h0a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v0a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  );
}
