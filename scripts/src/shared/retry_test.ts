import { assertEquals, assertRejects } from "jsr:@std/assert@1";
import { retry, RetryError } from "./retry.ts";

Deno.test("retry returns the first successful result", async () => {
  let calls = 0;
  const value = await retry(() => {
    calls++;
    return Promise.resolve("done");
  });
  assertEquals([value, calls], ["done", 1]);
});

Deno.test("retry stops at a non-retriable error and rethrows it", async () => {
  const failure = new Error("fatal");
  let calls = 0;
  const error = await assertRejects(() =>
    retry(() => {
      calls++;
      throw failure;
    }, { isRetriable: () => false })
  );
  assertEquals([error, calls], [failure, 1]);
});

Deno.test("retry reports the last error after the attempt limit", async () => {
  const failure = new Error("flaky");
  let calls = 0;
  const error = await assertRejects(
    () =>
      retry(() => {
        calls++;
        throw failure;
      }, { maxAttempts: 1 }),
    RetryError,
    "Retrying exceeded the maxAttempts (1).",
  );
  assertEquals([error.cause, calls], [failure, 1]);
});
