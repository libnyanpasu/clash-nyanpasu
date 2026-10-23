import { assertEquals, assertRejects } from "jsr:@std/assert@1";
import { exitStatus, runCommand, spawnCommand } from "./command.ts";

Deno.test("runCommand captures output and a non-zero exit without throwing", async () => {
  const result = await runCommand("sh", {
    args: ["-c", "printf out; printf err >&2; exit 3"],
  });
  assertEquals(result.success, false);
  assertEquals(result.code, 3);
  assertEquals(new TextDecoder().decode(result.stdout), "out");
  assertEquals(new TextDecoder().decode(result.stderr), "err");
});

Deno.test("runCommand merges env into the parent environment unless cleared", async () => {
  const print = ["-c", 'printf "%s|%s" "$NYANPASU_A" "${HOME:-unset}"'];
  const merged = await runCommand("sh", {
    args: print,
    env: { NYANPASU_A: "a" },
  });
  assertEquals(
    new TextDecoder().decode(merged.stdout).startsWith("a|"),
    true,
  );
  assertEquals(
    new TextDecoder().decode(merged.stdout).endsWith("|unset"),
    false,
  );
  const cleared = await runCommand("sh", {
    args: print,
    env: { NYANPASU_A: "a" },
    clearEnv: true,
  });
  assertEquals(new TextDecoder().decode(cleared.stdout), "a|unset");
});

Deno.test("runCommand rejects when the program does not exist", async () => {
  await assertRejects(() => runCommand("nyanpasu-missing-program"));
});

Deno.test("spawnCommand feeds piped stdin and reports the exit status", async () => {
  const child = spawnCommand("sh", {
    args: ["-c", 'read line; test "$line" = ping'],
    stdin: "piped",
    stderr: "null",
  });
  child.stdin!.end("ping\n");
  assertEquals((await exitStatus(child)).success, true);
});

Deno.test("runCommand writes input to stdin and captures the output", async () => {
  const result = await runCommand("cat", { input: "ping\n" });
  assertEquals(result.success, true);
  assertEquals(result.stdout.toString("utf8"), "ping\n");
});

Deno.test("exitStatus maps a signal exit to 128 plus the signal number", async () => {
  const child = spawnCommand("sh", { args: ["-c", "kill -TERM $$"] });
  assertEquals(await exitStatus(child), { success: false, code: 143 });
});
