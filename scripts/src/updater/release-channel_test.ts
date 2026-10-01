import { assertEquals } from "jsr:@std/assert@1";
import { selectChannelRelease } from "./release-channel.ts";

const release = (tag_name: string, prerelease = false, draft = false) => ({
  tag_name,
  prerelease,
  draft,
});

Deno.test("stable excludes beta, nightly, and draft releases", () => {
  const releases = [
    release("v2.1.0-beta.2", true),
    release("pre-release", true),
    release("v2.0.0"),
    release("v1.6.0"),
    release("v3.0.0", false, true),
    release("v2.2.0-beta.1"),
    release("v2.3.0", true),
  ];
  assertEquals(selectChannelRelease(releases, "stable")?.tag_name, "v2.0.0");
  assertEquals(selectChannelRelease(releases, "beta")?.tag_name, "v2.3.0");
});

Deno.test("beta follows semantic version precedence, including newer stable releases", () => {
  const releases = [
    release("v2.0.0-beta.10", true),
    release("v2.0.0-beta.2", true),
    release("v1.9.0"),
  ];
  assertEquals(
    selectChannelRelease(releases, "beta")?.tag_name,
    "v2.0.0-beta.10",
  );
  releases.push(release("v2.0.0"));
  assertEquals(selectChannelRelease(releases, "beta")?.tag_name, "v2.0.0");
});

Deno.test("channel selection does not mutate input and handles missing releases", () => {
  const releases = [release("v2.0.0-beta.1", true)];
  assertEquals(selectChannelRelease(releases, "stable"), undefined);
  assertEquals(selectChannelRelease([], "beta"), undefined);
  assertEquals(releases, [release("v2.0.0-beta.1", true)]);
});
