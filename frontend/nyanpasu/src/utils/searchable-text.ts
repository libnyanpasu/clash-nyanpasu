/**
 * Every string in `value`, nested ones included, lowercased and kept apart,
 * so a lowercased term is searched in all of them with one `includes`: the
 * same strings `containsSearchTerm` walks, for values searched many times.
 */
export function searchableText(value: unknown): string {
  const strings: string[] = []

  const collect = (item: unknown) => {
    if (typeof item === 'string') {
      strings.push(item)
    } else if (item !== null && typeof item === 'object') {
      Object.values(item).forEach(collect)
    }
  }

  collect(value)

  // A separator no search term holds, so a match never spans two strings.
  return strings.join('\0').toLowerCase()
}
