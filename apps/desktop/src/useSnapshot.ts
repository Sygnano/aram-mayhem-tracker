import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Snapshot } from "./types";

/** Latest backend snapshot: fetched once, then pushed by the `snapshot` event (~4 Hz). */
export function useSnapshot(): Snapshot | null {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  useEffect(() => {
    let alive = true;
    invoke<Snapshot>("get_snapshot").then((s) => alive && setSnapshot(s)).catch(console.error);
    const unlisten = listen<Snapshot>("snapshot", (e) => alive && setSnapshot(e.payload));
    return () => {
      alive = false;
      unlisten.then((f) => f());
    };
  }, []);
  return snapshot;
}
