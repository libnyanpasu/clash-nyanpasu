import { assertEquals } from "jsr:@std/assert@1";
import { uploadAllFilesWithResults } from "./file-server.ts";

Deno.test("upload batch continues after a file exhausts retries", async () => {
  const files = ["first.dmg", "unavailable.deb", "last.rpm", "next.AppImage"];
  const attempted: string[] = [];
  const outcome = await uploadAllFilesWithResults(
    files,
    "test-token",
    "nightly/test",
    async (filePath) => {
      attempted.push(filePath);
      if (filePath === "unavailable.deb") {
        throw new Error("HTTP 503 from archive");
      }
      return {
        fileName: filePath,
        downloadUrl: `https://archive.example/${filePath}`,
      };
    },
  );

  assertEquals([...new Set(attempted)].sort(), [...files].sort());
  assertEquals(
    outcome.results.map(({ fileName }) => fileName).sort(),
    ["first.dmg", "last.rpm", "next.AppImage"].sort(),
  );
  assertEquals(outcome.failures.length, 1);
  assertEquals(outcome.failures[0].fileName, "unavailable.deb");
  assertEquals(
    outcome.failures[0].message.includes("HTTP 503"),
    true,
    outcome.failures[0].message,
  );
});
