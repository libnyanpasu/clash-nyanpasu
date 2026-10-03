import { assertEquals, assertStringIncludes } from "jsr:@std/assert@1";
import { buildReleaseMessage } from "./telegram-notify.ts";

Deno.test("release notification links to archive and the published prerelease tag", () => {
  const message = buildReleaseMessage({
    nightly: false,
    tag: "v2.0.0-beta.1",
    gitShortHash: "6088e57",
  });
  assertEquals(
    message,
    [
      "Clash Nyanpasu v2.0.0-beta.1 Released!",
      "",
      "GitHub Release:",
      "https://github.com/libnyanpasu/clash-nyanpasu/releases/tag/v2.0.0-beta.1",
      "",
      "Downloads:",
      "https://archive.nyanpasu.org/",
    ].join("\n"),
  );
});

Deno.test("nightly notification links to its run and archive without listing artifacts", () => {
  const message = buildReleaseMessage({
    nightly: true,
    tag: "pre-release",
    gitShortHash: "6088e57",
    workflowRunId: "37150990990",
  });
  assertStringIncludes(message, "Nightly Build 6088e57");
  assertStringIncludes(message, "actions/runs/37150990990");
  assertStringIncludes(message, "Downloads:\nhttps://archive.nyanpasu.org/");
  assertEquals(message.includes("releases/tag/"), false);
  assertEquals(message.length < 4096, true);
});

Deno.test("manual nightly notification can omit a workflow run", () => {
  const message = buildReleaseMessage({
    nightly: true,
    tag: "pre-release",
    gitShortHash: "6088e57",
  });
  assertEquals(message.includes("GitHub Actions:"), false);
  assertStringIncludes(message, "https://archive.nyanpasu.org/");
});
