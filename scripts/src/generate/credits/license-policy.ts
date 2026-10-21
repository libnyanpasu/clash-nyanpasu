import { parse as parseToml } from "jsr:@std/toml@1";
import parseSpdx from "npm:spdx-expression-parse@5.0.0";
import spdxLicenses from "npm:spdx-license-list@6.12.0/full.js";

export type SpdxNode =
  | { license: string; plus?: boolean; exception?: string }
  | { left: SpdxNode; conjunction: "and" | "or"; right: SpdxNode };

/** Parses an SPDX expression, returning null when it is not valid SPDX. */
export function parseLicense(expression: string): SpdxNode | null {
  try {
    return parseSpdx(expression) as SpdxNode;
  } catch {
    return null;
  }
}

/** The priority-ordered license list that `cargo-about` also enforces. */
export function readAcceptedLicenses(aboutToml: string): string[] {
  const { accepted } = parseToml(aboutToml);
  if (
    !Array.isArray(accepted) ||
    !accepted.every((id) => typeof id === "string")
  ) {
    throw new Error("about.toml must list `accepted` license identifiers");
  }
  return accepted;
}

/**
 * Picks the licenses satisfying an expression, preferring alternatives whose
 * least preferred license ranks highest in `accepted`, as `cargo-about` does.
 * Returns null when the expression cannot be satisfied.
 */
export function chooseLicenses(
  node: SpdxNode,
  accepted: string[],
): string[] | null {
  if ("license" in node) {
    const id = node.exception
      ? `${node.license} WITH ${node.exception}`
      : node.license;
    return accepted.includes(id) ? [id] : null;
  }

  const left = chooseLicenses(node.left, accepted);
  const right = chooseLicenses(node.right, accepted);
  if (node.conjunction === "and") {
    return left && right ? [...left, ...right] : null;
  }
  if (!left || !right) return left ?? right;

  const rank = (ids: string[]) =>
    Math.max(...ids.map((id) => accepted.indexOf(id)));
  return rank(left) <= rank(right) ? left : right;
}

/** Every license identifier an expression names, without exceptions. */
export function licenseIds(node: SpdxNode): string[] {
  if ("license" in node) return [node.license];
  return [...licenseIds(node.left), ...licenseIds(node.right)];
}

/** The SPDX reference text of a license, ignoring any `WITH` exception. */
export function standardLicenseText(id: string): string | undefined {
  const [license] = id.split(" WITH ");
  return (spdxLicenses as Record<string, { licenseText?: string }>)[license]
    ?.licenseText;
}
