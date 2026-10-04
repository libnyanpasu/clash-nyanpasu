import { assertEquals, assertThrows } from "jsr:@std/assert@1";
import {
  archiveReadyTargets,
  parseManagedNightlyDirectories,
  selectNightlyCleanupTargets,
} from "./cleanup-sourceforge-nightly.ts";

const oldBuild = "120-1-" + "a".repeat(40);
const currentBuild = "121-2-" + "b".repeat(40);
const newerBuild = "122-1-" + "c".repeat(40);

Deno.test("SourceForge cleanup only recognizes immutable managed nightly IDs", () => {
  assertEquals(
    parseManagedNightlyDirectories(`${oldBuild}\n${currentBuild}\n`),
    [oldBuild, currentBuild],
  );
  assertThrows(
    () => parseManagedNightlyDirectories(`${oldBuild}\nlatest\n`),
    Error,
    "Unexpected entry",
  );
});

Deno.test("nightly cleanup requires every IA target to be ready before deleting an old folder", async () => {
  const allReady = await archiveReadyTargets(
    currentBuild,
    "token",
    async (input) => {
      const buildId = String(input).split("/").at(-1)!;
      return new Response(JSON.stringify({ buildId, status: "ready" }));
    },
  );
  assertEquals(allReady.ready, true);
  assertEquals(allReady.missing, []);

  const pending = await archiveReadyTargets(
    currentBuild,
    "token",
    async (input) => {
      const buildId = String(input).split("/").at(-1)!;
      return new Response(JSON.stringify({ buildId, status: "pending" }));
    },
  );
  assertEquals(pending.ready, false);
  assertEquals(pending.missing.length, 6);
});

Deno.test("nightly cleanup retains the promoted newest build and removes older managed builds", () => {
  assertEquals(
    selectNightlyCleanupTargets([oldBuild, currentBuild], currentBuild),
    [oldBuild],
  );
  assertEquals(selectNightlyCleanupTargets([currentBuild], currentBuild), []);
  assertEquals(
    selectNightlyCleanupTargets(
      [oldBuild, currentBuild, newerBuild],
      currentBuild,
    ),
    null,
  );
  assertThrows(
    () => selectNightlyCleanupTargets([oldBuild], currentBuild),
    Error,
    "directory is missing",
  );
});
