import { useCallback, useEffect, useRef, useState } from 'react'
import { invokeMutation, invokeQuery } from '../ipc/query-options'
import { rpc } from '../ipc/rpc'

const LOCAL_CACHE_PREFIX = 'nyanpasu-kv-:'
/** Mirrors the `WEB_STORAGE_KEY_PREFIX` constant on the backend. */
const WEB_KEY_PREFIX = 'web:'

function getLocalCache<T>(
  key: string,
  defaultValue: T,
  migrate?: (value: unknown) => T,
): T {
  try {
    const raw = localStorage.getItem(LOCAL_CACHE_PREFIX + btoa(key))

    if (raw === null) {
      return defaultValue
    }

    const parsed: unknown = JSON.parse(raw)
    return migrate ? migrate(parsed) : (parsed as T)
  } catch {
    return defaultValue
  }
}

function setLocalCache<T>(key: string, value: T): void {
  try {
    localStorage.setItem(LOCAL_CACHE_PREFIX + btoa(key), JSON.stringify(value))
  } catch {
    // ignore quota / security errors
  }
}

function removeLocalCache(key: string): void {
  localStorage.removeItem(LOCAL_CACHE_PREFIX + btoa(key))
}

export interface UseKvStorageOptions<T> {
  /**
   * Called with the raw parsed value from the cache, backend, or events.
   * Use this to transform old data shapes into the current shape.
   */
  migrate?: (value: unknown) => T
}

/**
 * A `useState`-like hook backed by the Tauri/redb KV storage.
 *
 * - Reads the localStorage cache immediately so the UI has a value on first
 *   render without flickering.
 * - Fetches the authoritative value from the backend on mount; the backend
 *   wins unless a newer write or event overtakes that read.
 * - Listens for `StorageValueChangedEvent` so all open windows stay in sync.
 * - Writing calls the generated `rpc.mutations.setStorageItem` binding and
 *   optimistically updates local
 *   state; the subsequent backend event confirms the change.
 * - The setter returns whether the mutation was confirmed. Failed writes keep
 *   the optimistic value available for a consumer to display and retry.
 */
export function useKvStorage<T>(
  key: string,
  defaultValue: T,
  options?: UseKvStorageOptions<T>,
): readonly [
  T,
  (value: T | ((prev: T) => T)) => Promise<boolean>,
  {
    isLoading: boolean
    readError: unknown | null
    refresh: () => Promise<void>
  },
] {
  const [value, setValueState] = useState<T>(() =>
    getLocalCache(key, defaultValue, options?.migrate),
  )
  const [isLoading, setIsLoading] = useState(true)
  const [readError, setReadError] = useState<unknown | null>(null)
  const refreshRef = useRef<() => Promise<void>>(async () => {})
  const refreshStorage = useCallback(() => refreshRef.current(), [])

  // Stable refs to avoid stale closures
  const defaultValueRef = useRef(defaultValue)

  const valueRef = useRef(value)
  valueRef.current = value

  const migrateRef = useRef(options?.migrate)
  migrateRef.current = options?.migrate

  // Track pending writes so the echo event from the backend doesn't cause a
  // redundant re-render. The set stores the serialized JSON of each in-flight
  // write; when the confirming event arrives we just discard it.
  const pendingWritesRef = useRef<Set<string>>(new Set())
  const revisionRef = useRef(0)
  const activeWritesRef = useRef(0)

  const applyMigrate = useCallback((raw: unknown): T => {
    return migrateRef.current ? migrateRef.current(raw) : (raw as T)
  }, [])

  // The initial state already holds this key's cache.
  const cachedKeyRef = useRef(key)

  // When key changes: reset to local cache and re-fetch from backend
  useEffect(() => {
    if (cachedKeyRef.current !== key) {
      cachedKeyRef.current = key
      const cached = getLocalCache(
        key,
        defaultValueRef.current,
        migrateRef.current,
      )
      valueRef.current = cached
      setValueState(cached)
      setIsLoading(true)
    }

    let disposed = false
    const refresh = async () => {
      const revision = revisionRef.current
      const readingDuringWrite = activeWritesRef.current > 0
      setIsLoading(true)
      try {
        const result = await invokeQuery(rpc.queries.getStorageItem(key))
        if (disposed) return
        // A read started before/during a write must not replace its preview or
        // a newer event snapshot with an older backend response.
        if (
          readingDuringWrite ||
          activeWritesRef.current > 0 ||
          revision !== revisionRef.current
        ) {
          return
        }

        if (result.status === 'error') {
          throw result.error
        }

        if (result.data === null) {
          valueRef.current = defaultValueRef.current
          setValueState(defaultValueRef.current)
          removeLocalCache(key)
        } else {
          const migrated = applyMigrate(JSON.parse(result.data))
          // Usually the backend confirms the cached value; keeping the state
          // object then spares every reader a render.
          if (JSON.stringify(migrated) !== JSON.stringify(valueRef.current)) {
            valueRef.current = migrated
            setValueState(migrated)
            setLocalCache(key, migrated)
          }
        }
        setReadError(null)
      } catch (error) {
        if (disposed) return
        setReadError(error)
        console.error('[useKvStorage] read failed:', error)
      } finally {
        if (!disposed) setIsLoading(false)
      }
    }

    refreshRef.current = refresh

    const stopResync = rpc.listenResync(() => {
      refresh()
    })

    refresh()

    return () => {
      disposed = true
      stopResync()
    }
  }, [key, applyMigrate])

  // Listen for changes emitted from backend (any window).
  // The backend emits the raw storage key which includes the `web:` prefix.
  useEffect(() => {
    const unlistenPromise = rpc.events.storageValueChangedEvent.listen(
      (event) => {
        if (event.payload.key !== WEB_KEY_PREFIX + key) {
          return
        }

        if (event.payload.value === null) {
          revisionRef.current += 1
          pendingWritesRef.current.delete('null')
          valueRef.current = defaultValueRef.current
          setValueState(defaultValueRef.current)
          removeLocalCache(key)
        } else {
          // If this event is the echo of our own optimistic write, skip the
          // redundant setState to avoid an unnecessary re-render.
          // Note: the backend double-encodes the value in the event payload
          // (the stored JSON string is wrapped in another JSON string), so we
          // compare against the double-encoded form.
          if (pendingWritesRef.current.has(event.payload.value)) {
            pendingWritesRef.current.delete(event.payload.value)
            return
          }

          try {
            // The backend emits the stored value double-encoded: the raw stored
            // string (already valid JSON) is JSON-encoded again inside the event
            // payload. Parse once to get the inner string, then parse again to
            // get the actual value. Fall back to single-parse for backends that
            // emit the value without extra encoding.
            const firstParsed = JSON.parse(event.payload.value)
            const parsed =
              typeof firstParsed === 'string'
                ? JSON.parse(firstParsed)
                : firstParsed
            const migrated = applyMigrate(parsed)

            revisionRef.current += 1
            valueRef.current = migrated
            setValueState(migrated)
            setLocalCache(key, migrated)
          } catch {
            // ignore invalid JSON from event
          }
        }
      },
    )

    return () => {
      unlistenPromise.then((fn) => fn())
    }
  }, [key, applyMigrate])

  const setValue = useCallback(
    async (newValue: T | ((prev: T) => T)) => {
      const resolved =
        typeof newValue === 'function'
          ? (newValue as (prev: T) => T)(valueRef.current)
          : newValue

      const serialized = JSON.stringify(resolved)
      revisionRef.current += 1
      activeWritesRef.current += 1

      // Register this write so the confirming event can be suppressed.
      // The backend double-encodes the value in the event, so we store the
      // double-encoded form to match what the event listener will receive.
      pendingWritesRef.current.add(JSON.stringify(serialized))

      // Optimistic update — the backend event will also arrive and confirm
      valueRef.current = resolved
      setValueState(resolved)
      setLocalCache(key, resolved)

      try {
        const result = await invokeMutation(rpc.mutations.setStorageItem, [
          key,
          serialized,
        ])
        if (result.status === 'error') {
          throw result.error
        }

        return true
      } catch (error) {
        pendingWritesRef.current.delete(JSON.stringify(serialized))
        console.error('[useKvStorage] setStorageItem failed:', error)

        return false
      } finally {
        activeWritesRef.current -= 1
      }
    },
    [key],
  )

  return [
    value,
    setValue,
    { isLoading, readError, refresh: refreshStorage },
  ] as const
}

/**
 * Debug utilities for the backend KV store.
 * Not intended for production use — these bypass per-key subscriptions.
 */
export const kvStorageDebug = {
  /** Returns all stored key-value pairs with values deserialized from JSON. */
  async getAll(): Promise<Record<string, unknown>> {
    const result = await invokeQuery(rpc.queries.getAllStorageItems())

    if (result.status === 'error') {
      throw result.error
    }

    return Object.fromEntries(
      result.data.map(({ key, value }) => {
        try {
          return [key, JSON.parse(value)]
        } catch {
          return [key, value]
        }
      }),
    )
  },

  /** Removes every entry from the backend storage. */
  async clear(): Promise<void> {
    const result = await invokeMutation(rpc.mutations.clearStorage, [])

    if (result.status === 'error') {
      throw result.error
    }
  },
}
