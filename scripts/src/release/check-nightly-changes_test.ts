import { assertEquals, assertRejects } from "jsr:@std/assert@1";
import {
  checkNightlyChanges,
  isNightlyBuildInput,
} from "./check-nightly-changes.ts";

Deno.test("nightly inputs exclude documentation and dedicated tests", () => {
  for (
    const file of [
      "docs/design/config.json",
      "README.md",
      "AGENTS.md",
      "frontend/ui/tests/drawer.browser.test.tsx",
      "scripts/src/release/publish_test.ts",
      "frontend/ui/src/__tests__/fixture.ts",
    ]
  ) assertEquals(isNightlyBuildInput(file), false, file);

  for (
    const file of [
      "frontend/nyanpasu/src/main.tsx",
      "backend/tauri/src/main.rs",
      "backend/nyanpasu-runtime",
      "manifest/version.json",
      "pnpm-lock.yaml",
      "backend/Cargo.lock",
      ".github/workflows/deps-build-linux.yaml",
      "scripts/src/release/prepare-nightly.ts",
      "backend/tauri/resources/icon.png",
    ]
  ) assertEquals(isNightlyBuildInput(file), true, file);
});

async function withRepository(
  test: (
    cwd: string,
    git: (...args: string[]) => Promise<void>,
  ) => Promise<void>,
) {
  const cwd = await Deno.makeTempDir();
  const git = async (...args: string[]) => {
    const result = await new Deno.Command("git", {
      cwd,
      args,
      stdout: "piped",
      stderr: "piped",
    }).output();
    if (!result.success) {
      throw new Error(new TextDecoder().decode(result.stderr));
    }
  };
  try {
    await git("init");
    await git("config", "user.email", "nightly@example.invalid");
    await git("config", "user.name", "Nightly Test");
    await Deno.writeTextFile(`${cwd}/app.ts`, "original\n");
    await git("add", "app.ts");
    await git("-c", "commit.gpgsign=false", "commit", "-m", "Initial input");
    await test(cwd, git);
  } finally {
    await Deno.remove(cwd, { recursive: true });
  }
}

Deno.test("first nightly builds; identical and documentation-only trees skip", async () => {
  await withRepository(async (cwd, git) => {
    assertEquals((await checkNightlyChanges(cwd)).shouldBuild, true);
    await git("tag", "pre-release");
    assertEquals((await checkNightlyChanges(cwd)).shouldBuild, false);

    await Deno.writeTextFile(`${cwd}/README.md`, "Documentation\n");
    await Deno.writeTextFile(`${cwd}/app_test.ts`, "Test\n");
    await git("add", "README.md", "app_test.ts");
    await git("-c", "commit.gpgsign=false", "commit", "-m", "Docs and tests");
    assertEquals((await checkNightlyChanges(cwd)).shouldBuild, false);

    await Deno.writeTextFile(`${cwd}/app.ts`, "changed\n");
    await git("add", "app.ts");
    await git("-c", "commit.gpgsign=false", "commit", "-m", "Change app");
    assertEquals((await checkNightlyChanges(cwd)).shouldBuild, true);

    await Deno.writeTextFile(`${cwd}/app.ts`, "original\n");
    await git("add", "app.ts");
    await git("-c", "commit.gpgsign=false", "commit", "-m", "Revert app");
    assertEquals((await checkNightlyChanges(cwd)).shouldBuild, false);
  });
});

Deno.test("annotated baseline tags work and renaming code into docs builds", async () => {
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
    assertEquals((await checkNightlyChanges(cwd)).shouldBuild, false);
    await git("mv", "app.ts", "README.md");
    await git("-c", "commit.gpgsign=false", "commit", "-m", "Move app to docs");
    assertEquals((await checkNightlyChanges(cwd)).shouldBuild, true);
  });
});

Deno.test("deleting a build input triggers a nightly", async () => {
  await withRepository(async (cwd, git) => {
    await git("tag", "pre-release");
    await git("rm", "app.ts");
    await git("-c", "commit.gpgsign=false", "commit", "-m", "Delete app");
    assertEquals((await checkNightlyChanges(cwd)).shouldBuild, true);
  });
});

Deno.test("Git errors fail the check rather than reporting no changes", async () => {
  const cwd = await Deno.makeTempDir();
  try {
    await assertRejects(() => checkNightlyChanges(cwd));
  } finally {
    await Deno.remove(cwd, { recursive: true });
  }
});
