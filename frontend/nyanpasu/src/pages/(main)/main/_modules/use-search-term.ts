import { useIsPresent } from 'motion/react'
import { useEffect, useRef, useState } from 'react'
import { useDebounce } from '@uidotdev/usehooks'

/**
 * A page's search field, written into its entry's `q` once typing pauses so
 * returning to the entry finds it. `writeQuery` replaces the entry's `q`.
 */
export function useSearchTerm(
  q: string | undefined,
  writeQuery: (q: string | undefined) => void,
) {
  const [search, setSearch] = useState(q ?? '')

  // The `q` this page last wrote or adopted.
  const known = useRef(q)

  // Another `q` comes from moving between the page's own entries; the field
  // follows it instead of writing its older term over it.
  useEffect(() => {
    if (q !== known.current) {
      known.current = q
      setSearch(q ?? '')
    }
  }, [q])

  const debouncedSearch = useDebounce(search, 300)

  // A page sliding out still renders, while the router already shows the next
  // entry: a write from it would replace that entry.
  const isPresent = useIsPresent()

  useEffect(() => {
    const next = debouncedSearch || undefined

    // Waits until the field settles, which an adopted `q` also has to.
    if (!isPresent || debouncedSearch !== search || next === known.current) {
      return
    }

    known.current = next
    writeQuery(next)
  }, [debouncedSearch, search, isPresent, writeQuery])

  return [search, setSearch] as const
}
