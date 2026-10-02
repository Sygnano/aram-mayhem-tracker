/**
 * Parses the two large anvil documents off the main thread: the ~7 MB map bin and the ~31 MB string
 * table. `JSON.parse` on those would stall every other request for as long as it takes, so this
 * runs on a worker thread, once per patch and locale, and exits afterwards so its heap goes with it.
 *
 * `parseAnvilDocuments` is the caller's side; the rest of the file is the worker's.
 */

import { isMainThread, parentPort, Worker, workerData } from "node:worker_threads";
import { type RawShard, shardsFromMap, stringsFromTable } from "./anvils.ts";

interface Input {
  map: Uint8Array;
  table: Uint8Array;
}

export interface ParsedAnvils {
  raw: RawShard[];
  /** Only the names the shards ask for. */
  names: Map<string, string>;
}

/** Runs both parses on a fresh worker thread. */
export function parseAnvilDocuments(map: Uint8Array, table: Uint8Array): Promise<ParsedAnvils> {
  return new Promise((resolve, reject) => {
    // The same file, as `.ts` when Node runs the source and `.js` once built.
    const worker = new Worker(new URL(import.meta.url), { workerData: { map, table } });
    let settled = false;
    worker.once("message", (parsed: ParsedAnvils) => {
      settled = true;
      resolve(parsed);
    });
    worker.once("error", (error) => {
      settled = true;
      reject(new Error(`parsing the anvil documents: ${error.message}`, { cause: error }));
    });
    worker.once("exit", (code) => {
      if (!settled) {
        reject(new Error(`the anvil parsing worker exited with code ${code} and no result`));
      }
    });
  });
}

function parse({ map, table }: Input): ParsedAnvils {
  const decoder = new TextDecoder();
  const raw = shardsFromMap(JSON.parse(decoder.decode(map)));
  const wanted = new Set(raw.map((r) => r.nameKey));
  const names = stringsFromTable(JSON.parse(decoder.decode(table)), wanted);
  return { raw, names };
}

if (!isMainThread && parentPort !== null) {
  // A throw here surfaces as the worker's `error` event on the caller's side.
  parentPort.postMessage(parse(workerData as Input));
}
