/**
 * The whole dataset in one response (D-090): every champion's bundle, the champion table and the
 * anvil rankings. The app downloads it at startup and every 24 hours, keeps it on disk, and needs
 * nothing else from the service about champions while it runs.
 *
 * It is about 113 MB of JSON and 13 MB gzipped on 16.19, far too much to rebuild per request. It is
 * built once per data version and rankings save, straight into a gzip stream one champion at a time
 * so the whole uncompressed text never sits in memory, and the gzipped bytes are kept and served as
 * they are. The ETag names what it was built from, so an app that already has it gets a 304.
 */

import { createHash } from "node:crypto";
import { createGzip } from "node:zlib";
import type { Logger } from "pino";
import type { Data } from "./data.ts";
import { SCHEMA_VER, type Store } from "./db/store.ts";
import { upstreamStatus } from "./errors.ts";
import { savedRankings } from "./routes/anvils.ts";
import { championInfo } from "./routes/augments.ts";
import { championPools } from "./routes/bundle.ts";
import { championTableBody } from "./routes/champions.ts";
import { currentVersion } from "./routes/status.ts";

/** Bumped when the dataset's shape changes, so every app downloads it again. */
export const DATASET_VER = 1;

export interface BuiltDataset {
  /** Quoted, as it goes in the header. */
  etag: string;
  gzip: Buffer;
  dataPath: string;
  champions: number;
}

export interface DatasetOptions {
  store: Store;
  data: Data;
  log: Logger;
}

export class Datasets {
  readonly #store: Store;
  readonly #data: Data;
  readonly #log: Logger;
  /** The last one built. A newer version or rankings save replaces it. */
  #built: BuiltDataset | null = null;
  /** Builds in progress by ETag, so a burst of requests on a cold dataset builds it once. */
  readonly #building = new Map<string, Promise<BuiltDataset>>();

  constructor({ store, data, log }: DatasetOptions) {
    this.#store = store;
    this.#data = data;
    this.#log = log;
  }

  /** The ETag the dataset for the served version and the current rankings has, built or not. */
  currentEtag(): string {
    const { dataPath } = currentVersion(this.#store);
    const savedAt = this.#store.anvilRankings()?.savedAt ?? 0;
    const hash = createHash("sha256")
      .update(`${DATASET_VER}:${SCHEMA_VER}:${dataPath}:${savedAt}`)
      .digest("hex")
      .slice(0, 32);
    return `"${hash}"`;
  }

  /** The dataset for the served version, building it if it is not built yet. */
  get(): Promise<BuiltDataset> {
    const etag = this.currentEtag();
    if (this.#built?.etag === etag) {
      return Promise.resolve(this.#built);
    }
    let pending = this.#building.get(etag);
    if (pending === undefined) {
      pending = this.#build(etag)
        .then((built) => {
          this.#built = built;
          return built;
        })
        .finally(() => this.#building.delete(etag));
      this.#building.set(etag, pending);
    }
    return pending;
  }

  /** Builds ahead of the first request. A failure is logged; the next request tries again. */
  warm(): void {
    this.get().catch((error: unknown) =>
      this.#log.error({ err: error }, "could not build the dataset"),
    );
  }

  async #build(etag: string): Promise<BuiltDataset> {
    const started = performance.now();
    const version = currentVersion(this.#store);
    const { dataPath } = version;
    const [table, global, catalogue] = await Promise.all([
      this.#data.championTable(dataPath),
      this.#data.globalRankings(dataPath),
      this.#data.catalogue(version.version),
    ]);
    const ids = Object.keys(table.champions)
      .map(Number)
      .sort((a, b) => a - b);

    const gzip = createGzip();
    const chunks: Buffer[] = [];
    gzip.on("data", (chunk: Buffer) => chunks.push(chunk));
    const finished = new Promise<void>((resolve, reject) => {
      gzip.on("end", resolve);
      gzip.on("error", reject);
    });
    // `write` buffers whatever it is given; waiting on `drain` keeps that to one champion at a time.
    const write = async (text: string) => {
      if (!gzip.write(text)) {
        await new Promise((resolve) => gzip.once("drain", resolve));
      }
    };

    await write(
      `{"patch":${JSON.stringify(version.version)},"dataDate":${JSON.stringify(version.dataDate)},"champions":{`,
    );
    let written = 0;
    const absent: number[] = [];
    for (const id of ids) {
      let champion: Awaited<ReturnType<Data["championAugments"]>>;
      let archetypes: Awaited<ReturnType<Data["championBuilds"]>>;
      try {
        [champion, archetypes] = await Promise.all([
          this.#data.championAugments(dataPath, id),
          this.#data.championBuilds(dataPath, id),
        ]);
      } catch (error) {
        // Ranked but without details upstream, which the crawler recorded: nothing to send.
        if (upstreamStatus(error) === 404) {
          absent.push(id);
          continue;
        }
        throw error;
      }
      const bundle = {
        champion: championInfo(champion),
        pools: championPools(champion, global, catalogue),
        archetypes,
      };
      await write(
        `${written === 0 ? "" : ","}${JSON.stringify(String(id))}:${JSON.stringify(bundle)}`,
      );
      written += 1;
    }
    await write(`},"championTable":${JSON.stringify(championTableBody(table))}`);
    await write(`,"anvilRankings":${JSON.stringify(savedRankings(this.#store))}}`);
    gzip.end();
    await finished;

    const body = Buffer.concat(chunks);
    this.#log.info(
      {
        dataPath,
        champions: written,
        absent,
        gzippedBytes: body.length,
        ms: Math.round(performance.now() - started),
      },
      "dataset built",
    );
    return { etag, gzip: body, dataPath, champions: written };
  }
}
