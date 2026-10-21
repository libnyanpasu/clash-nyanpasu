export type CreditSource = "cargo" | "npm" | "bundled";
export type ComponentKind = "sidecar" | "resource" | "data";
export type Platform = "windows" | "linux" | "macos";

/** One credited package before its license texts are deduplicated. */
export interface CreditEntry {
  source: CreditSource;
  name: string;
  version?: string;
  homepage?: string;
  /** SPDX expression; absent for bundled components without a license. */
  license?: string;
  texts: string[];
  kind?: ComponentKind;
  platforms?: Platform[];
  notes?: string;
}

export interface CreditPackage extends Omit<CreditEntry, "texts"> {
  textIds: string[];
}

/** The generated data the frontend reads; keep its copy in sync. */
export interface OpenSourceCredits {
  schemaVersion: 1;
  texts: Record<string, string>;
  packages: CreditPackage[];
}

const SOURCE_ORDER: CreditSource[] = ["bundled", "cargo", "npm"];

function normalizeText(text: string): string {
  return text.replace(/\r\n?/g, "\n").replace(/[ \t]+$/gm, "").trim();
}

async function textId(text: string): Promise<string> {
  const digest = await crypto.subtle.digest(
    "SHA-256",
    new TextEncoder().encode(text),
  );
  return Array.from(new Uint8Array(digest).slice(0, 8))
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}

function compare(a: string | undefined, b: string | undefined): number {
  const left = a ?? "";
  const right = b ?? "";
  return left < right ? -1 : left > right ? 1 : 0;
}

/**
 * Deduplicates license texts and orders everything, so the same inputs always
 * produce the same document.
 */
export async function buildCreditsDocument(
  entries: CreditEntry[],
): Promise<OpenSourceCredits> {
  const texts = new Map<string, string>();
  const packages: CreditPackage[] = [];

  for (const { texts: entryTexts, ...entry } of entries) {
    const textIds: string[] = [];
    for (const text of entryTexts.map(normalizeText).filter(Boolean)) {
      const id = await textId(text);
      texts.set(id, text);
      if (!textIds.includes(id)) textIds.push(id);
    }
    packages.push({ ...entry, textIds });
  }

  packages.sort((a, b) =>
    SOURCE_ORDER.indexOf(a.source) - SOURCE_ORDER.indexOf(b.source) ||
    compare(a.name.toLowerCase(), b.name.toLowerCase()) ||
    compare(a.name, b.name) || compare(a.version, b.version)
  );
  return {
    schemaVersion: 1,
    texts: Object.fromEntries([...texts].sort(([a], [b]) => compare(a, b))),
    packages,
  };
}
