import { assertEquals, assertThrows } from "jsr:@std/assert@1";
import { standardLicenseText } from "./license-policy.ts";
import {
  type ManualComponent,
  manualEntries,
  parseManualCredits,
  sidecarCoverage,
} from "./manual.ts";

function component(
  overrides: Partial<ManualComponent> = {},
): ManualComponent {
  return {
    name: "core",
    kind: "sidecar",
    homepage: "https://example.test/core",
    ...overrides,
  };
}

function entries(
  components: ManualComponent[],
  files: Record<string, string> = {},
) {
  return manualEntries(components, (path) => files[path]);
}

Deno.test("the manual list rejects unknown fields and values", () => {
  const parsed = parseManualCredits(`
[[component]]
name = "core"
kind = "sidecar"
homepage = "https://example.test/core"
`);
  assertEquals(parsed.component.length, 1);
  assertEquals(parsed["npm-override"], []);

  for (
    const invalid of [
      '[[component]]\nname = "a"\nkind = "binary"\nhomepage = "https://a.test"',
      '[[component]]\nname = "a"\nkind = "data"\nhomepage = "not a url"',
      '[[component]]\nname = "a"\nkind = "data"\nhomepage = "https://a.test"\nlicence = "MIT"',
      '[[npm-override]]\nname = "a"\nlicense = "MIT"',
    ]
  ) {
    assertThrows(() => parseManualCredits(invalid), Error, "components.toml");
  }
});

Deno.test("components without a license only declare their address", () => {
  const { entries: result, problems } = entries([
    component({ bundles: ["sidecar/core"], notes: "Closed-source." }),
  ]);
  assertEquals(problems, []);
  assertEquals(result, [{
    source: "bundled",
    name: "core",
    license: undefined,
    texts: [],
    kind: "sidecar",
    homepage: "https://example.test/core",
    notes: "Closed-source.",
  }]);
});

Deno.test("license texts come from files or the SPDX reference", () => {
  const { entries: result, problems } = entries([
    component({ name: "a", license: "MIT OR ISC" }),
    component({
      name: "b",
      license: "LicenseRef-Custom",
      "license-files": ["licenses/b.txt"],
    }),
  ], { "licenses/b.txt": "custom terms" });
  assertEquals(problems, []);
  assertEquals(result.map((entry) => entry.texts), [
    [standardLicenseText("MIT"), standardLicenseText("ISC")],
    ["custom terms"],
  ]);
});

Deno.test("invalid components are reported by name", () => {
  const { problems } = entries([
    component({ name: "dup" }),
    component({ name: "dup" }),
    component({ name: "files", "license-files": ["licenses/x.txt"] }),
    component({ name: "bad", license: "not a license" }),
    component({
      name: "missing",
      license: "MIT",
      "license-files": ["licenses/missing.txt"],
    }),
    component({ name: "custom", license: "LicenseRef-Custom" }),
  ]);
  assertEquals(problems, [
    "component dup is declared twice",
    "component files has license-files but no license",
    "component bad license not a license is not valid SPDX",
    "component missing misses licenses/missing.txt",
    "component custom needs license-files for LicenseRef-Custom",
  ]);
});

Deno.test("sidecar coverage matches externalBin in both directions", () => {
  const components = [
    component({ bundles: ["sidecar/core", "sidecar/core-alpha"] }),
    component({ name: "old", bundles: ["sidecar/removed"] }),
  ];
  assertEquals(
    sidecarCoverage(
      ["sidecar/core", "sidecar/core-alpha", "sidecar/new"],
      components,
    ),
    [
      "sidecar/new is packaged but no component lists it in bundles",
      "bundles lists sidecar/removed, which tauri.conf.json does not package",
    ],
  );
});
