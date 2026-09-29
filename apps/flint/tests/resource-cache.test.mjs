import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { transform } from "esbuild";

const source = await readFile(new URL("../src/resource-cache.ts", import.meta.url), "utf8");
const { code } = await transform(source, { loader: "ts", format: "esm" });
const { ResourceCache } = await import(`data:text/javascript;base64,${Buffer.from(code).toString("base64")}`);

test("returning to an in-flight native read reuses its work after the previous page leaves", { timeout: 2000 }, async () => {
  const cache = new ResourceCache();
  let reads = 0;
  let finish;
  let signal;
  const read = (abort) => {
    reads += 1;
    signal = abort;
    return new Promise((resolve) => { finish = resolve; });
  };
  const first = cache.acquire("preview:blender:1");
  const pending = cache.read(first, read);
  await Promise.resolve();
  cache.release(first);
  assert.equal(signal.aborted, true);
  const second = cache.acquire("preview:blender:1");
  const reused = cache.read(second, read);
  await Promise.resolve();
  assert.equal(reads, 1);
  finish("image");
  assert.equal(await pending, "image");
  assert.equal(await reused, "image");
  cache.release(second);
  assert.equal(cache.peek("preview:blender:1").data, "image");
});

test("leaving before a read starts cancels it, and a later visit can read normally", { timeout: 2000 }, async () => {
  const cache = new ResourceCache();
  let reads = 0;
  const read = async () => { reads += 1; return "result"; };
  const entry = cache.acquire("workflows");
  const pending = cache.read(entry, read);
  cache.release(entry);
  await assert.rejects(pending, { name: "AbortError" });
  assert.equal(reads, 0);
  const returned = cache.acquire("workflows");
  assert.equal(await cache.read(returned, read), "result");
  assert.equal(reads, 1);
  cache.release(returned);
});

test("a shared read stops only after its last consumer leaves and preserves its last successful value", { timeout: 2000 }, async () => {
  const cache = new ResourceCache();
  const first = cache.acquire("workflows");
  await cache.read(first, async () => "previous");
  const second = cache.acquire("workflows");
  let signal;
  const pending = cache.read(first, (abort) => {
    signal = abort;
    return new Promise((_, reject) => {
      abort.addEventListener("abort", () => reject(abort.reason), { once: true });
    });
  });
  await Promise.resolve();
  cache.release(first);
  assert.equal(signal.aborted, false);
  cache.release(second);
  await assert.rejects(pending, { name: "AbortError" });
  assert.equal(cache.peek("workflows").data, "previous");
});

test("cache eviction removes inactive results without duplicating active requests", { timeout: 2000 }, async () => {
  const cache = new ResourceCache(2);
  let finish;
  const active = cache.acquire("active");
  const request = cache.read(active, () => new Promise((resolve) => { finish = resolve; }));
  for (const key of ["older", "newer"]) {
    const entry = cache.acquire(key);
    await cache.read(entry, async () => key);
    cache.release(entry);
  }
  assert.equal(cache.peek("older"), undefined);
  assert.equal(cache.peek("newer").data, "newer");
  const second = cache.acquire("active");
  const shared = cache.read(second, () => { throw new Error("Duplicate native read"); });
  finish("completed");
  assert.equal(await request, "completed");
  assert.equal(await shared, "completed");
  cache.release(active);
  cache.release(second);
});
