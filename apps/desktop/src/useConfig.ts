import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AppConfig } from "./types";

/**
 * The saved settings, and a way to change some of them. A change shows at once and is written
 * straight away; only the fields passed are touched, so a checkbox saves itself.
 *
 * If the backend refuses, the promise rejects with its reason and the settings are read back, so
 * what is shown is what was actually kept rather than what was asked for.
 */
export function useConfig(): [AppConfig | null, (patch: Partial<AppConfig>) => Promise<void>] {
  const [cfg, setCfg] = useState<AppConfig | null>(null);
  useEffect(() => {
    invoke<AppConfig>("get_config").then(setCfg).catch(console.error);
  }, []);
  const update = useCallback((patch: Partial<AppConfig>) => {
    setCfg((c) => c && { ...c, ...patch });
    // An empty directory is how the backend is told to forget it.
    const settings = "leagueDir" in patch ? { ...patch, leagueDir: patch.leagueDir ?? "" } : patch;
    return invoke<void>("save_settings", { settings }).catch((e) => {
      invoke<AppConfig>("get_config").then(setCfg).catch(console.error);
      throw e;
    });
  }, []);
  return [cfg, update];
}
