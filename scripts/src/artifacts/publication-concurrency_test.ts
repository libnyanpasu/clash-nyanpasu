import { assertEquals, assertRejects } from "jsr:@std/assert@1";
import { runPublicationTasks } from "./publication-concurrency.ts";

Deno.test("publication tasks bound concurrency, drain failures and preserve input order", async () => {
  const release = Promise.withResolvers<void>();
  const started = Promise.withResolvers<void>();
  let active = 0;
  let maximum = 0;
  const seen: number[] = [];
  const result = runPublicationTasks([0, 1, 2, 3, 4], 2, async (value) => {
    active++;
    maximum = Math.max(maximum, active);
    seen.push(value);
    if (seen.length === 2) started.resolve();
    await release.promise;
    active--;
    if (value === 1) throw new Error("target failed");
    return value * 2;
  });
  await started.promise;
  assertEquals(seen, [0, 1]);
  release.resolve();
  const results = await result;
  assertEquals(maximum, 2);
  assertEquals(seen, [0, 1, 2, 3, 4]);
  assertEquals(results.map((entry) => entry.status), [
    "fulfilled",
    "rejected",
    "fulfilled",
    "fulfilled",
    "fulfilled",
  ]);
  assertEquals(results[4], { status: "fulfilled", value: 8 });
  assertEquals(await runPublicationTasks([], 2, async () => 0), []);
  await assertRejects(() => runPublicationTasks([0], 0, async () => 0));
});
