import { useEffect, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { currentLocale, setLanguage, subscribeLocale, type Language } from "./i18n";
import type { AppConfig } from "./types";

/** Each webview subscribes, so the overlay and companion change together. */
export function useLanguage() {
  const locale = useSyncExternalStore(subscribeLocale, currentLocale);
  useEffect(() => {
    let alive = true;
    let revision = 0;
    const unlisten = listen<Language>("language-changed", (event) => {
      if (alive) {
        revision++;
        setLanguage(event.payload);
      }
    });
    unlisten.then(async () => {
      const requestedRevision = revision;
      const config = await invoke<AppConfig>("get_config");
      // A late initial read must not undo a more recent change from another webview.
      if (alive && revision === requestedRevision) setLanguage(config.language);
    }).catch(console.error);
    return () => { alive = false; unlisten.then((stop) => stop()); };
  }, []);
  return locale;
}
