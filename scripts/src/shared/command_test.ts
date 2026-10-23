import assert from "node:assert/strict";
import { test } from "node:test";
import { exitStatus, runCommand, spawnCommand } from "./command.ts";

test("runCommand captures output and a non-zero exit without throwing", async () => {
  const result = await runCommand("sh", {
    args: ["-c", "printf out; printf err >&2; exit 3"],
  });
  assert.deepStrictEqual(result.success, false);
  assert.deepStrictEqual(result.code, 3);
  assert.deepStrictEqual(new TextDecoder().decode(result.stdout), "out");
  assert.deepStrictEqual(new TextDecoder().decode(result.stderr), "err");
});

test("runCommand merges env into the parent environment unless cleared", async () => {
  // eslint-disable-next-line no-template-curly-in-string -- shell parameter expansion
  const print = ["-c", 'printf "%s|%s" "$NYANPASU_A" "${HOME:-unset}"'];
  const merged = await runCommand("sh", {
    args: print,
    env: { NYANPASU_A: "a" },
  });
  assert.deepStrictEqual(
    new TextDecoder().decode(merged.stdout).startsWith("a|"),
    true,
  );
  assert.deepStrictEqual(
    new TextDecoder().decode(merged.stdout).endsWith("|unset"),
    false,
  );
  const cleared = await runCommand("sh", {
    args: print,
    env: { NYANPASU_A: "a" },
    clearEnv: true,
  });
  assert.deepStrictEqual(new TextDecoder().decode(cleared.stdout), "a|unset");
});

test("runCommand rejects when the program does not exist", async () => {
  await assert.rejects(() => runCommand("nyanpasu-missing-program"));
});

test("spawnCommand feeds piped stdin and reports the exit status", async () => {
  const child = spawnCommand("sh", {
    args: ["-c", 'read line; test "$line" = ping'],
    stdin: "piped",
    stderr: "null",
  });
  child.stdin!.end("ping\n");
  assert.deepStrictEqual((await exitStatus(child)).success, true);
});

test("runCommand writes input to stdin and captures the output", async () => {
  const result = await runCommand("cat", { input: "ping\n" });
  assert.deepStrictEqual(result.success, true);
  assert.deepStrictEqual(result.stdout.toString("utf8"), "ping\n");
});

test("exitStatus maps a signal exit to 128 plus the signal number", async () => {
  const child = spawnCommand("sh", { args: ["-c", "kill -TERM $$"] });
  assert.deepStrictEqual(await exitStatus(child), { success: false, code: 143 });
});
