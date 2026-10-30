import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  type PropsWithChildren,
} from 'react'
import {
  hashKey,
  skipToken,
  useQueryClient,
  type QueryClient,
} from '@tanstack/react-query'
import { invokeMutation } from '../ipc/query-options'
import {
  EMPTY_KV_STORAGE_ENTRY,
  kvStorageEventSnapshot,
  kvStorageQueryKey,
  mergeKvStorageSnapshot,
  type KvStorageEntry,
} from './kv-storage-state'
import { useQueryApi } from './rpc-provider'

type QueryApi = ReturnType<typeof useQueryApi>

const LOCAL_CACHE_PREFIX = 'nyanpasu-kv-:'

/** The last value this browser profile showed for `key`, as JSON text. */
export function readKvStorageLocalCache(key: string): string | null {
  try {
    return localStorage.getItem(LOCAL_CACHE_PREFIX + btoa(key))
  } catch {
    return null
  }
}

function mirrorKvStorageLocalCache(key: string, entry: KvStorageEntry) {
  const value = entry.draft?.value ?? entry.value
  if (value === undefined) return
  try {
    if (value === null) localStorage.removeItem(LOCAL_CACHE_PREFIX + btoa(key))
    else localStorage.setItem(LOCAL_CACHE_PREFIX + btoa(key), value)
  } catch {
    // ignore quota / security errors
  }
}

type KvStorageStore = ReturnType<typeof createKvStorageStore>

/**
 * Owns the storage values of one query client: each key is read once, kept
 * current by the backend's events, and written through one queue per key so
 * every hook of a key shows the same value.
 */
function createKvStorageStore(api: QueryApi, queryClient: QueryClient) {
  let markReady = () => {}
  // The first reads wait for the listeners, so nothing committed after a read
  // goes unheard.
  const ready = new Promise<void>((resolve) => {
    markReady = resolve
  })
  const reads = new Map<string, Promise<void>>()
  const writeTails = new Map<string, Promise<boolean>>()
  let draftSequence = 0

  const peek = (key: string) =>
    queryClient.getQueryData<KvStorageEntry>(kvStorageQueryKey(key))

  const update = (
    key: string,
    change: (entry: KvStorageEntry) => KvStorageEntry,
  ) => {
    const entry = queryClient.setQueryData<KvStorageEntry>(
      kvStorageQueryKey(key),
      (entry) => change(entry ?? EMPTY_KV_STORAGE_ENTRY),
    )
    if (entry) mirrorKvStorageLocalCache(key, entry)
  }

  // `isCurrent` turns false once a newer read of the key has started, which
  // then settles the key instead.
  const settleRead = async (key: string, isCurrent: () => boolean) => {
    await ready
    try {
      const result = await api.getStorageItem(key)
      if (!isCurrent()) return
      if (result.status === 'error') throw result.error
      const entry = peek(key) ?? EMPTY_KV_STORAGE_ENTRY
      const merged = mergeKvStorageSnapshot(entry, result.data)
      // Unreadable stored JSON is a failed read, not a default to save over.
      if (merged !== entry && merged.value != null) JSON.parse(merged.value)
      update(key, () => ({ ...merged, read: { status: 'done', error: null } }))
    } catch (error) {
      if (!isCurrent()) return
      console.error('[useKvStorage] read failed:', error)
      update(key, (entry) => ({ ...entry, read: { status: 'done', error } }))
    }
  }

  const read = (key: string) => {
    update(key, (entry) => ({
      ...entry,
      read: { status: 'reading', error: entry.read.error },
    }))
    const isCurrent = () => reads.get(key) === promise
    const promise: Promise<void> = settleRead(key, isCurrent).finally(() => {
      if (isCurrent()) reads.delete(key)
    })
    reads.set(key, promise)
    return promise
  }

  // Values missed in a gap may have been committed after a read in flight
  // was answered, so a resync starts fresh reads.
  const resync = () => {
    for (const query of queryClient
      .getQueryCache()
      .findAll({ queryKey: ['kvStorage'] })) {
      read(query.queryKey[1] as string)
    }
  }

  return {
    peek,

    /** Calls `onChange`, in the same task, whenever the entry of `key` changes. */
    subscribe: (key: string, onChange: () => void) => {
      const queryHash = hashKey(kvStorageQueryKey(key))
      return queryClient.getQueryCache().subscribe((event) => {
        if (event.query.queryHash === queryHash) onChange()
      })
    },

    /** Reads `key` unless this client has read it or is reading it. */
    ensureRead: (key: string) => {
      if ((peek(key)?.read.status ?? 'idle') === 'idle') read(key)
    },

    refresh: (key: string) => reads.get(key) ?? read(key),

    /**
     * Shows `value` at once and stores it after the earlier writes of `key`.
     * Resolves to whether the backend confirmed it; a failed value stays
     * shown until a newer value is committed.
     */
    write: (key: string, value: string): Promise<boolean> => {
      draftSequence += 1
      const id = draftSequence
      update(key, (entry) => ({
        ...entry,
        draft: { id, value, failed: false },
      }))

      const operation = (writeTails.get(key) ?? Promise.resolve(true)).then(
        async () => {
          try {
            const result = await invokeMutation(api.mutations.setStorageItem, [
              key,
              value,
            ])
            if (result.status === 'error') throw result.error
            update(key, (entry) => {
              const merged = mergeKvStorageSnapshot(entry, {
                value,
                revision: result.data,
              })
              return merged.draft?.id === id
                ? { ...merged, draft: null }
                : merged
            })
            return true
          } catch (error) {
            console.error('[useKvStorage] setStorageItem failed:', error)
            update(key, (entry) =>
              entry.draft?.id === id
                ? { ...entry, draft: { ...entry.draft, failed: true } }
                : entry,
            )
            return false
          }
        },
      )
      writeTails.set(key, operation)
      return operation
    },

    connect: () => {
      // A connection replaced by a newer one, as StrictMode does, neither
      // applies events nor lets the first reads start.
      let disposed = false
      const onResync = () => {
        if (!disposed) resync()
      }
      const registrations = [
        api.events.storageValueChangedEvent.listen(({ payload }) => {
          if (disposed) return
          const change = kvStorageEventSnapshot(payload)
          // A key this client never read is read when it is first used.
          if (!change || !peek(change.key)) return
          update(change.key, (entry) =>
            mergeKvStorageSnapshot(entry, change.snapshot),
          )
        }),
        api.events.storageResyncEvent.listen(onResync),
      ]
      const stopResync = api.listenResync(onResync)
      Promise.allSettled(registrations).then((results) => {
        if (disposed) return
        for (const result of results) {
          if (result.status === 'rejected') {
            console.error('[useKvStorage] listen failed:', result.reason)
          }
        }
        markReady()
      })

      return () => {
        disposed = true
        stopResync()
        for (const registration of registrations) {
          registration.then((stop) => stop()).catch(console.error)
        }
      }
    },
  }
}

const KvStorageContext = createContext<KvStorageStore | null>(null)

export const KvStorageProvider = ({ children }: PropsWithChildren) => {
  const api = useQueryApi()
  const queryClient = useQueryClient()

  const store = useMemo(() => {
    // Only the store's reads and events change these values, and a key read
    // once is kept for as long as the window lives.
    queryClient.setQueryDefaults(['kvStorage'], {
      queryFn: skipToken,
      gcTime: Infinity,
    })
    return createKvStorageStore(api, queryClient)
  }, [api, queryClient])

  useEffect(() => store.connect(), [store])

  return (
    <KvStorageContext.Provider value={store}>
      {children}
    </KvStorageContext.Provider>
  )
}

export const useKvStorageStore = () => {
  const store = useContext(KvStorageContext)
  if (!store) {
    throw new Error('useKvStorage must be used within a KvStorageProvider')
  }
  return store
}
