import { setTimeout as sleep } from "node:timers/promises";
import { describe, expect, it } from "vitest";
import { aramkitKey } from "../db/store.ts";
import { upstreamStatus } from "../errors.ts";
import { fakeUpstream, testService } from "../test/helpers.ts";
import { AUGMENT_RANKINGS, CHAMPION_DETAILS } from "./documents.ts";

async function setup() {
  const upstream = await fakeUpstream({
    "/slow": { json: {}, delayMs: 300 },
    "/flaky": { status: 503 },
  });
  const { documents, store } = await testService({ upstreamBase: upstream.base });
  return { upstream, documents, store };
}

describe("upstream documents", () => {
  it("finish a fetch whose first caller gave up, and hand it to the next one", async () => {
    const { upstream, documents } = await setup();
    const key = aramkitKey("data/x", AUGMENT_RANKINGS);
    const url = `${upstream.base}/slow`;

    // The first caller gives up long before upstream answers, as a client disconnecting would.
    const abandoned = await Promise.race([
      documents.ensure(key, url).then(() => "answered"),
      sleep(50).then(() => "gave up"),
    ]);
    expect(abandoned).toBe("gave up");

    // The next one gets the document from the fetch the first one started.
    const body = await documents.ensure(key, url);
    expect(body.toString()).toBe("{}");
    expect(upstream.hits("/slow"), "the abandoned fetch was not reused").toBe(1);
    expect(documents.inFlight.size).toBe(0);
  });

  it("share one fetch between concurrent callers", async () => {
    const { upstream, documents } = await setup();
    const key = aramkitKey("data/x", AUGMENT_RANKINGS);

    const bodies = await Promise.all(
      Array.from({ length: 8 }, () => documents.ensure(key, `${upstream.base}/slow`)),
    );
    expect(bodies.map(String)).toEqual(Array(8).fill("{}"));
    expect(upstream.hits("/slow")).toBe(1);
  });

  it("give every caller a 404 and do not refetch it", async () => {
    const { upstream, documents } = await setup();
    const key = aramkitKey("data/x", CHAMPION_DETAILS, "999999");

    for (let i = 0; i < 3; i++) {
      const error = await documents
        .ensure(key, `${upstream.base}/missing`)
        .catch((e: unknown) => e);
      expect(upstreamStatus(error)).toBe(404);
    }
    expect(upstream.hits("/missing"), "a 404 is honoured for an hour").toBe(1);
    expect(documents.inFlight.size).toBe(0);
  });

  it("retry a 5xx three times, then hold the failure briefly", async () => {
    const { upstream, documents, store } = await setup();
    const key = aramkitKey("data/x", AUGMENT_RANKINGS);

    const error = await documents.ensure(key, `${upstream.base}/flaky`).catch((e: unknown) => e);
    expect(upstreamStatus(error)).toBe(503);
    expect(upstream.hits("/flaky")).toBe(3);

    // Held: the next request answers from the record without asking upstream.
    const again = await documents.ensure(key, `${upstream.base}/flaky`).catch((e: unknown) => e);
    expect(upstreamStatus(again)).toBe(503);
    expect(upstream.hits("/flaky")).toBe(3);
    expect(store.failureBackoff(key)?.status).toBe(503);
  });
});
