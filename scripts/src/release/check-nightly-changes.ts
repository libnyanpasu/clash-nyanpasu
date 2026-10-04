import { WORKSPACE_ROOT } from "../shared/repo-paths.ts";

export function isNightlyBuildInput(file: string): boolean {
  return !(
    file.startsWith("docs/") ||
    /\.(md|mdx|rst)$/i.test(file) ||
    /(^|\/)(tests|__tests__)\//.test(file) ||
    /(_test|\.test|\.spec)\.[^/]+$/.test(file)
  );
}

async function git(cwd: string, args: string[]) {
  const result = await new Deno.Command("git", {
    cwd,
    args,
    stdout: "piped",
    stderr: "piped",
  }).output();
  return {
    code: result.code,
    stdout: new TextDecoder().decode(result.stdout),
    stderr: new TextDecoder().decode(result.stderr),
  };
}

export async function checkNightlyChanges(cwd: string) {
  const tag = "refs/tags/pre-release";
  const baseline = await git(cwd, ["show-ref", "--verify", "--quiet", tag]);
  if (baseline.code === 1) {
    return { shouldBuild: true, reason: "No previous nightly tag exists." };
  }
  if (baseline.code !== 0) throw new Error(baseline.stderr);

  // Compare final trees, including deletions and both sides of renames.
  // A change reverted since the last nightly does not require another build.
  const diff = await git(cwd, [
    "diff",
    "--name-only",
    "--no-renames",
    "-z",
    `${tag}^{commit}`,
    "HEAD",
    "--",
  ]);
  if (diff.code !== 0) throw new Error(diff.stderr);

  const inputs = diff.stdout.split("\0").filter(Boolean).filter(
    isNightlyBuildInput,
  );
  return {
    shouldBuild: inputs.length > 0,
    reason: inputs.length > 0
      ? `${inputs.length} build input(s) changed since pre-release.`
      : "No substantive changes since pre-release (documentation and tests are excluded).",
  };
}

if (import.meta.main) {
  const result = await checkNightlyChanges(WORKSPACE_ROOT);
  console.log(result.reason);

  const output = Deno.env.get("GITHUB_OUTPUT");
  if (output) {
    await Deno.writeTextFile(output, `should_build=${result.shouldBuild}\n`, {
      append: true,
    });
  }

  const summary = Deno.env.get("GITHUB_STEP_SUMMARY");
  if (summary) {
    await Deno.writeTextFile(
      summary,
      `## Nightly build check\n\n${
        result.shouldBuild ? "Build" : "Skip"
      }: ${result.reason}\n`,
      { append: true },
    );
  }
}
