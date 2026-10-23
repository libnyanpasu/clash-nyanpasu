import assert from "node:assert/strict";
import { test } from "node:test";
import { selectChannelRelease } from "./release-channel.ts";

const release = (tag_name: string, prerelease = false, draft = false) => ({
  tag_name,
  prerelease,
  draft,
});

test("stable excludes beta, nightly, and draft releases", () => {
  const releases = [
    release("v2.1.0-beta.2", true),
    release("pre-release", true),
    release("v2.0.0"),
    release("v1.6.0"),
    release("v3.0.0", false, true),
    release("v2.2.0-beta.1"),
    release("v2.3.0", true),
  ];
  assert.deepStrictEqual(selectChannelRelease(releases, "stable")?.tag_name, "v2.0.0");
  assert.deepStrictEqual(selectChannelRelease(releases, "beta")?.tag_name, "v2.3.0");
});

test("beta follows semantic version precedence, including newer stable releases", () => {
  const releases = [
    release("v2.0.0-beta.10", true),
    release("v2.0.0-beta.2", true),
    release("v1.9.0"),
  ];
  assert.deepStrictEqual(
    selectChannelRelease(releases, "beta")?.tag_name,
    "v2.0.0-beta.10",
  );
  releases.push(release("v2.0.0"));
  assert.deepStrictEqual(selectChannelRelease(releases, "beta")?.tag_name, "v2.0.0");
});

test("channel selection does not mutate input and handles missing releases", () => {
  const releases = [release("v2.0.0-beta.1", true)];
  assert.deepStrictEqual(selectChannelRelease(releases, "stable"), undefined);
  assert.deepStrictEqual(selectChannelRelease([], "beta"), undefined);
  assert.deepStrictEqual(releases, [release("v2.0.0-beta.1", true)]);
});
