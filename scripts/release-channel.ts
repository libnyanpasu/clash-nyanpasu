import semver from "npm:semver";

export type ReleaseChannel = "stable" | "beta";

export function selectChannelRelease<
  T extends {
    tag_name: string;
    draft: boolean;
    prerelease: boolean;
  },
>(releases: T[], channel: ReleaseChannel): T | undefined {
  return releases.filter((release) => {
    const version = semver.valid(release.tag_name);
    return !release.draft && version &&
      (channel === "beta" ||
        (!release.prerelease && semver.prerelease(version) === null));
  }).sort((a, b) => semver.rcompare(a.tag_name, b.tag_name))[0];
}
