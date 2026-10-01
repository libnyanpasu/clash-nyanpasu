import { useMemo } from 'react'
import {
  keepPreviousData,
  useInfiniteQuery,
  useQuery,
} from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import {
  commands,
  type Dimension,
  type TrafficQuery,
  type UsageCursor,
} from './rpc-bindings'

const PAGE_SIZE = 50

/**
 * A short digest of `keys`, in order: 53 bits of cyrb53 over every key, with
 * a separator so moving a character between keys changes it.
 */
export function digestKeys(keys: readonly string[]): string {
  let h1 = 0xdeadbeef
  let h2 = 0x41c6ce57

  const mix = (code: number) => {
    h1 = Math.imul(h1 ^ code, 2654435761)
    h2 = Math.imul(h2 ^ code, 1597334677)
  }

  for (const key of keys) {
    for (let i = 0; i < key.length; i++) {
      mix(key.charCodeAt(i))
    }

    mix(0)
  }

  h1 = Math.imul(h1 ^ (h1 >>> 16), 2246822507)
  h1 ^= Math.imul(h2 ^ (h2 >>> 13), 3266489909)
  h2 = Math.imul(h2 ^ (h2 >>> 16), 2246822507)
  h2 ^= Math.imul(h1 ^ (h1 >>> 13), 3266489909)

  const hash = 4294967296 * (2097151 & h2) + (h1 >>> 0)

  return `${keys.length}:${hash.toString(36)}`
}

/**
 * Traffic of the given groups within `query`, in request order; groups without
 * traffic are left out.
 */
export function useTrafficUsageByKeys(
  query: TrafficQuery,
  dimension: Dimension,
  keys: string[],
  options?: { refetchInterval?: number | false },
) {
  // React Query hashes the whole query key on every render, serializing every
  // key each time: thousands of rule labels for every connection sample. The
  // query key holds a digest computed once per array instead.
  const digest = useMemo(() => digestKeys(keys), [keys])

  return useQuery({
    queryKey: ['traffic-usage-by-keys', query, dimension, digest],
    queryFn: async () =>
      unwrapResult(
        await commands.queryTrafficUsageByKeys(query, dimension, keys),
      ),
    enabled: keys.length > 0,
    // New keys (another filter or config) keep showing the previous answer
    // until theirs arrives.
    placeholderData: keepPreviousData,
    refetchInterval: options?.refetchInterval ?? 2000,
    // Unavailable recording fails every time; the next poll retries anyway.
    retry: false,
  })
}

/**
 * Every group of `dimension` within `query`, heaviest first, one page at a
 * time. Not polled: a listing that moves under the reader would skip and
 * repeat rows.
 */
export function useTrafficUsagePages(
  query: TrafficQuery,
  dimension: Dimension,
) {
  return useInfiniteQuery({
    queryKey: ['traffic-usage-pages', query, dimension],
    initialPageParam: null as UsageCursor | null,
    queryFn: async ({ pageParam }) =>
      unwrapResult(
        await commands.queryTrafficUsage(
          query,
          dimension,
          pageParam,
          PAGE_SIZE,
        ),
      ),
    getNextPageParam: (page) => page.next ?? undefined,
    retry: false,
  })
}
