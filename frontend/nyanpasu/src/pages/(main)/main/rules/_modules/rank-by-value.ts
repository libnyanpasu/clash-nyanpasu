/**
 * `entries`, kept in rule order, with those valued above zero moved to the
 * front, highest first: the order a full sort by value then rule order gives,
 * while sorting only the few entries that have a value.
 */
export function rankByValue<T extends { index: number }>(
  entries: readonly T[],
  value: (entry: T) => number,
): T[] {
  const ranked: Array<[entry: T, value: number]> = []
  const rest: T[] = []

  for (const entry of entries) {
    const entryValue = value(entry)

    if (entryValue > 0) {
      ranked.push([entry, entryValue])
    } else {
      rest.push(entry)
    }
  }

  ranked.sort(
    ([a, aValue], [b, bValue]) => bValue - aValue || a.index - b.index,
  )

  return [...ranked.map(([entry]) => entry), ...rest]
}
