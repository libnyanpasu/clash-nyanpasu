import assert from "node:assert/strict";
import { test } from "node:test";
import {
  checkNightlyChanges,
  isNightlyBuildInput,
} from "./check-nightly-changes.ts";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runCommand } from "../shared/command.ts";

test("nightly inputs exclude documentation and dedicated tests", () => {
  for (
    const file of [
      "docs/design/config.json",
      "README.md",
      "AGENTS.md",
      "frontend/ui/tests/drawer.browser.test.tsx",
      "scripts/src/release/publish_test.ts",
      "frontend/ui/src/__tests__/fixture.ts",
    ]
  ) assert.deepStrictEqual(isNightlyBuildInput(file), false, file);

  for (
    const file of [
      "frontend/nyanpasu/src/main.tsx",
      "backend/tauri/src/main.rs",
      "backend/nyanpasu-runtime",
      "backend/enable-loopback",
      "manifest/version.json",
      "pnpm-lock.yaml",
      "backend/Cargo.lock",
      ".github/workflows/deps-build-linux.yaml",
      "scripts/src/release/prepare-nightly.ts",
      "backend/tauri/resources/icon.png",
    ]
  ) assert.deepStrictEqual(isNightlyBuildInput(file), true, file);
});

async function withRepository(
  test: (
    cwd: string,
    git: (...args: string[]) => Promise<void>,
  ) => Promise<void>,
) {
  const cwd = await mkdtemp(join(tmpdir(), "nyanpasu-scripts-"));
  const git = async (...args: string[]) => {
    const result = await runCommand("git", {
      cwd,
      args,
      stdout: "piped",
      stderr: "piped",
    });
    if (!result.success) {
      throw new Error(result.stderr.toString("utf8"));
    }
  };
  try {
    await git("init");
    await git("config", "user.email", "nightly@example.invalid");
    await git("config", "user.name", "Nightly Test");
    await writeFile(`${cwd}/app.ts`, "original\n");
    await git("add", "app.ts");
    await git("-c", "commit.gpgsign=false", "commit", "-m", "Initial input");
    await test(cwd, git);
  } finally {
    await rm(cwd, { recursive: true });
  }
}

test("first nightly builds; identical and documentation-only trees skip", async () => {
  await withRepository(async (cwd, git) => {
    assert.deepStrictEqual((await checkNightlyChanges(cwd)).shouldBuild, true);
    await git("tag", "pre-release");
    assert.deepStrictEqual((await checkNightlyChanges(cwd)).shouldBuild, false);

    await writeFile(`${cwd}/README.md`, "Documentation\n");
    await writeFile(`${cwd}/app_test.ts`, "Test\n");
    await git("add", "README.md", "app_test.ts");
    await git("-c", "commit.gpgsign=false", "commit", "-m", "Docs and tests");
    assert.deepStrictEqual((await checkNightlyChanges(cwd)).shouldBuild, false);

    await writeFile(`${cwd}/app.ts`, "changed\n");
    await git("add", "app.ts");
    await git("-c", "commit.gpgsign=false", "commit", "-m", "Change app");
    assert.deepStrictEqual((await checkNightlyChanges(cwd)).shouldBuild, true);

    await writeFile(`${cwd}/app.ts`, "original\n");
    await git("add", "app.ts");
    await git("-c", "commit.gpgsign=false", "commit", "-m", "Revert app");
    assert.deepStrictEqual((await checkNightlyChanges(cwd)).shouldBuild, false);
  });
});

test("annotated baseline tags work and renaming code into docs builds", async () => {
  await withRepository(async (cwd, git) => {
    await git(
      "-c",
      "tag.gpgsign=false",
      "tag",
      "-a",
      "pre-release",
      "-m",
      "Nightly",
    );
    assert.deepStrictEqual((await checkNightlyChanges(cwd)).shouldBuild, false);
    await git("mv", "app.ts", "README.md");
    await git("-c", "commit.gpgsign=false", "commit", "-m", "Move app to docs");
    assert.deepStrictEqual((await checkNightlyChanges(cwd)).shouldBuild, true);
  });
});

test("deleting a build input triggers a nightly", async () => {
  await withRepository(async (cwd, git) => {
    await git("tag", "pre-release");
    await git("rm", "app.ts");
    await git("-c", "commit.gpgsign=false", "commit", "-m", "Delete app");
    assert.deepStrictEqual((await checkNightlyChanges(cwd)).shouldBuild, true);
  });
});

test("Git errors fail the check rather than reporting no changes", async () => {
  const cwd = await mkdtemp(join(tmpdir(), "nyanpasu-scripts-"));
  try {
    await assert.rejects(() => checkNightlyChanges(cwd));
  } finally {
    await rm(cwd, { recursive: true });
  }
});
