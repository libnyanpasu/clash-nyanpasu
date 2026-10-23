import assert from "node:assert/strict";
import { test } from "node:test";
import { buildReleaseMessage } from "./telegram-notify.ts";

test("release notification downloads use the published GitHub prerelease tag", () => {
  const message = buildReleaseMessage({
    nightly: false,
    tag: "v2.0.0-beta.1",
    gitShortHash: "6088e57",
  });
  assert.deepStrictEqual(
    message,
    [
      "Clash Nyanpasu v2.0.0-beta.1 Released!",
      "",
      "GitHub Release:",
      "https://github.com/libnyanpasu/clash-nyanpasu/releases/tag/v2.0.0-beta.1",
      "",
      "Downloads:",
      "https://github.com/libnyanpasu/clash-nyanpasu/releases/tag/v2.0.0-beta.1",
    ].join("\n"),
  );
});

test("nightly notification links to its run and GitHub release without listing artifacts", () => {
  const message = buildReleaseMessage({
    nightly: true,
    tag: "pre-release",
    gitShortHash: "6088e57",
    workflowRunId: "37150990990",
  });
  assert.ok(message.includes("Nightly Build 6088e57"));
  assert.ok(message.includes("actions/runs/37150990990"));
  assert.ok(message.includes("Downloads:\nhttps://github.com/libnyanpasu/clash-nyanpasu/releases/tag/pre-release"));
  assert.deepStrictEqual(message.includes("archive.nyanpasu.org"), false);
  assert.deepStrictEqual(message.length < 4096, true);
});

test("manual nightly notification can omit a workflow run", () => {
  const message = buildReleaseMessage({
    nightly: true,
    tag: "pre-release",
    gitShortHash: "6088e57",
  });
  assert.deepStrictEqual(message.includes("GitHub Actions:"), false);
  assert.ok(message.includes("https://github.com/libnyanpasu/clash-nyanpasu/releases/tag/pre-release"));
});
