import { assertEquals } from "jsr:@std/assert@1";
import { join } from "jsr:@std/path";
import { standardLicenseText } from "./license-policy.ts";
import {
  npmEntries,
  type NpmPackage,
  type PackageFiles,
  readNpmPackages,
} from "./npm.ts";

const ACCEPTED = ["MIT", "Apache-2.0", "Unlicense"];

function pkg(overrides: Partial<NpmPackage> = {}): NpmPackage {
  return {
    name: "lib",
    version: "1.0.0",
    license: "MIT",
    texts: ["MIT text"],
    ...overrides,
  };
}

Deno.test("installed packages are read once per version", async () => {
  const dirs: Record<string, Record<string, string>> = {
    [join("store", "a@1")]: {
      "package.json": '{"version":"1.0.0"}',
      "LICENSE": "a license",
      "NOTICE.md": "a notice",
      "README.md": "readme",
    },
    [join("store", "a@1_peer")]: {
      "package.json": '{"version":"1.0.0"}',
      "LICENSE": "a license",
    },
    [join("store", "a@2")]: { "package.json": '{"version":"2.0.0"}' },
  };
  const files: PackageFiles = {
    readTextFile: (file) =>
      Promise.resolve(dirs[join(file, "..")][file.split(/[\\/]/).at(-1)!]),
    readDirNames: (dir) => Promise.resolve(Object.keys(dirs[dir])),
  };

  const packages = await readNpmPackages({
    MIT: [{
      name: "a",
      license: "MIT",
      homepage: "https://a.test",
      paths: Object.keys(dirs),
    }],
  }, files);
  assertEquals(packages, [
    {
      name: "a",
      version: "1.0.0",
      license: "MIT",
      homepage: "https://a.test",
      texts: ["a license", "a notice"],
    },
    {
      name: "a",
      version: "2.0.0",
      license: "MIT",
      homepage: "https://a.test",
      texts: [],
    },
  ]);
});

Deno.test("overrides correct undeclared licenses", () => {
  const { entries, problems } = npmEntries(
    [pkg({ license: "Unknown", texts: ["Unlicense text"] })],
    [{
      name: "lib",
      version: "1.0.0",
      license: "Unlicense",
      notes: "checked",
    }],
    ACCEPTED,
  );
  assertEquals(problems, []);
  assertEquals(entries[0].license, "Unlicense");
  assertEquals(entries[0].notes, "checked");
});

Deno.test("packages without license files use the chosen reference text", () => {
  const { entries } = npmEntries(
    [pkg({ license: "(MPL-2.0 OR Apache-2.0)", texts: [] })],
    [],
    ACCEPTED,
  );
  assertEquals(entries[0].texts, [standardLicenseText("Apache-2.0")]);
});

Deno.test("unlicensed, unaccepted and stale entries are reported", () => {
  const { entries, problems } = npmEntries(
    [
      pkg({ name: "unknown", license: "Unknown" }),
      pkg({ name: "gpl", license: "GPL-2.0-only" }),
      pkg({ name: "custom", license: "MIT AND LicenseRef-X", texts: [] }),
    ],
    [{ name: "gone", version: "1.0.0", license: "MIT" }],
    [...ACCEPTED, "LicenseRef-X"],
  );
  assertEquals(entries, []);
  assertEquals(problems, [
    "npm: unknown@1.0.0 declares no valid license (Unknown); add an [[npm-override]]",
    "npm: gpl@1.0.0 license GPL-2.0-only is not accepted",
    "npm: custom@1.0.0 ships no license file for MIT AND LicenseRef-X",
    "[[npm-override]] gone@1.0.0 matches no installed package",
  ]);
});
