import assert from "node:assert/strict";
import { test } from "node:test";
import { buildCreditsDocument, type CreditEntry } from "./document.ts";

const ENTRIES: CreditEntry[] = [
  { source: "npm", name: "zod", version: "4.0.0", texts: ["MIT text\r\n"] },
  {
    source: "cargo",
    name: "serde",
    version: "1.0.0",
    texts: ["MIT text  \n", "NOTICE"],
  },
  { source: "bundled", name: "mihomo", texts: [] },
  { source: "cargo", name: "Anyhow", version: "1.0.0", texts: ["MIT text"] },
];

test("license texts are normalized and stored once", async () => {
  const document = await buildCreditsDocument(ENTRIES);
  assert.deepStrictEqual(Object.values(document.texts).sort(), ["MIT text", "NOTICE"]);

  const [mit] = document.packages.find((pkg) => pkg.name === "zod")!.textIds;
  assert.deepStrictEqual(
    document.packages.find((pkg) => pkg.name === "serde")!.textIds[0],
    mit,
  );
});

test("documents are ordered and identical for the same inputs", async () => {
  const document = await buildCreditsDocument(ENTRIES);
  assert.deepStrictEqual(
    document.packages.map((pkg) => `${pkg.source}:${pkg.name}`),
    ["bundled:mihomo", "cargo:Anyhow", "cargo:serde", "npm:zod"],
  );
  assert.deepStrictEqual(
    JSON.stringify(await buildCreditsDocument([...ENTRIES].reverse())),
    JSON.stringify(document),
  );
});
