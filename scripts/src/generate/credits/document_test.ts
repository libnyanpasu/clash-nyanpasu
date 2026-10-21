import { assertEquals } from "jsr:@std/assert@1";
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

Deno.test("license texts are normalized and stored once", async () => {
  const document = await buildCreditsDocument(ENTRIES);
  assertEquals(Object.values(document.texts).sort(), ["MIT text", "NOTICE"]);

  const [mit] = document.packages.find((pkg) => pkg.name === "zod")!.textIds;
  assertEquals(
    document.packages.find((pkg) => pkg.name === "serde")!.textIds[0],
    mit,
  );
});

Deno.test("documents are ordered and identical for the same inputs", async () => {
  const document = await buildCreditsDocument(ENTRIES);
  assertEquals(
    document.packages.map((pkg) => `${pkg.source}:${pkg.name}`),
    ["bundled:mihomo", "cargo:Anyhow", "cargo:serde", "npm:zod"],
  );
  assertEquals(
    JSON.stringify(await buildCreditsDocument([...ENTRIES].reverse())),
    JSON.stringify(document),
  );
});
