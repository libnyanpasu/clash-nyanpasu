/** Collect static slots, including both literal branches of a JSX conditional. */
export function extractDataSlots(source: string): string[] {
  const slots = new Set<string>();
  for (const match of source.matchAll(/\bdata-slot\s*=\s*["']([^"']+)["']/g)) {
    slots.add(match[1]);
  }
  for (
    const match of source.matchAll(
      /\bdata-slot\s*=\s*\{[^{}]*?\?\s*(['"])([^'"]+)\1\s*:\s*(['"])([^'"]+)\3\s*\}/g,
    )
  ) {
    slots.add(match[2]);
    slots.add(match[4]);
  }
  return [...slots];
}
