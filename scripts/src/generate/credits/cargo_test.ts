import { assertEquals } from "jsr:@std/assert@1";
import { type CargoAboutOutput, cargoEntries } from "./cargo.ts";

function crate(
  name: string,
  overrides: Partial<CargoAboutOutput["crates"][number]["package"]> = {},
): CargoAboutOutput["crates"][number] {
  return {
    package: {
      id: `${name}-id`,
      name,
      version: "1.0.0",
      source: "registry+https://github.com/rust-lang/crates.io-index",
      license: "MIT OR Apache-2.0",
      homepage: null,
      repository: `https://example.test/${name}`,
      manifest_path: `/registry/${name}/Cargo.toml`,
      ...overrides,
    },
    license: "MIT OR Apache-2.0",
  };
}

Deno.test("crates keep their texts and notices; local crates are skipped", () => {
  const about: CargoAboutOutput = {
    crates: [
      crate("serde", { license: "MIT  OR  Apache-2.0" }),
      crate("own", { source: null }),
      crate("git-dep", { license: null }),
    ],
    licenses: [
      {
        text: "Apache text",
        used_by: [{ crate: { id: "serde-id" } }, { crate: { id: "own-id" } }],
      },
      { text: "MIT text", used_by: [{ crate: { id: "git-dep-id" } }] },
    ],
  };

  const { entries, problems } = cargoEntries(
    about,
    new Map([["serde-id", ["serde notice"]]]),
  );
  assertEquals(problems, []);
  assertEquals(entries, [
    {
      source: "cargo",
      name: "serde",
      version: "1.0.0",
      homepage: "https://example.test/serde",
      license: "MIT OR Apache-2.0",
      texts: ["Apache text", "serde notice"],
    },
    {
      source: "cargo",
      name: "git-dep",
      version: "1.0.0",
      homepage: "https://example.test/git-dep",
      license: "MIT OR Apache-2.0",
      texts: ["MIT text"],
    },
  ]);
});

Deno.test("crates without a license text are reported", () => {
  const { problems } = cargoEntries(
    { crates: [crate("bare")], licenses: [] },
    new Map(),
  );
  assertEquals(problems, [
    "cargo: bare@1.0.0 has no license text (MIT OR Apache-2.0)",
  ]);
});
