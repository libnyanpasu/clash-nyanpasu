import { useCallback, useEffect, useRef, useState } from 'react'
import { commands, events } from '../ipc/bindings'

const LOCAL_CACHE_PREFIX = 'nyanpasu-kv-:'
/** Mirrors the `WEB_STORAGE_KEY_PREFIX` constant on the backend. */
const WEB_KEY_PREFIX = 'web:'

function getLocalCache<T>(key: string, defaultValue: T): T {
  try {
    const raw = localStorage.getItem(LOCAL_CACHE_PREFIX + btoa(key))

    if (raw === null) {
      return defaultValue
    }

    return JSON.parse(raw) as T
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
   * Called with the raw parsed value when it is loaded from the backend.
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
 *   always wins.
 * - Listens for `StorageValueChangedEvent` so all open windows stay in sync.
 * - Writing calls `commands.setStorageItem` and optimistically updates local
 *   state; the subsequent backend event confirms the change.
 */
export function useKvStorage<T>(
  key: string,
  defaultValue: T,
  options?: UseKvStorageOptions<T>,
): readonly [
  T,
  (value: T | ((prev: T) => T)) => Promise<void>,
  {
    isLoading: boolean
  },
] {
  const [value, setValueState] = useState<T>(() =>
    getLocalCache(key, defaultValue),
  )
  const [isLoading, setIsLoading] = useState(true)

  // Stable refs to avoid stale closures
  const defaultValueRef = useRef(defaultValue)

  const valueRef = useRef(value)
  valueRef.current = value

  const migrateRef = useRef(options?.migrate)
  migrateRef.current = options?.migrate

  const applyMigrate = useCallback((raw: unknown): T => {
    return migrateRef.current ? migrateRef.current(raw) : (raw as T)
  }, [])

  // When key changes: reset to local cache and re-fetch from backend
  useEffect(() => {
    setValueState(getLocalCache(key, defaultValueRef.current))
    setIsLoading(true)

    commands.getStorageItem(key).then((result) => {
      if (result.status === 'ok') {
        if (result.data !== null) {
          try {
            const parsed = JSON.parse(result.data)
            const migrated = applyMigrate(parsed)
            setValueState(migrated)
            setLocalCache(key, migrated)
          } catch {
            // backend returned non-JSON; keep local cache
          }
        }

        setIsLoading(false)
      }
    })
  }, [key, applyMigrate])

  // Listen for changes emitted from backend (any window).
  // The backend emits the raw storage key which includes the `web:` prefix.
  useEffect(() => {
    const unlistenPromise = events.storageValueChangedEvent.listen((event) => {
      if (event.payload.key !== WEB_KEY_PREFIX + key) {
        return
      }

      if (event.payload.value === null) {
        setValueState(defaultValueRef.current)
        removeLocalCache(key)
      } else {
        try {
          const parsed = JSON.parse(event.payload.value)
          const migrated = applyMigrate(parsed)

          setValueState(migrated)
          setLocalCache(key, migrated)
        } catch {
          // ignore invalid JSON from event
        }
      }
    })

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

      // Optimistic update — the backend event will also arrive and confirm
      setValueState(resolved)
      setLocalCache(key, resolved)

      const result = await commands.setStorageItem(
        key,
        JSON.stringify(resolved),
      )

      if (result.status === 'error') {
        console.error('[useKvStorage] setStorageItem failed:', result.error)
      }
    },
    [key],
  )

  return [value, setValue, { isLoading }] as const
}

/**
 * Debug utilities for the backend KV store.
 * Not intended for production use — these bypass per-key subscriptions.
 */
export const kvStorageDebug = {
  /** Returns all stored key-value pairs with values deserialized from JSON. */
  async getAll(): Promise<Record<string, unknown>> {
    const result = await commands.getAllStorageItems()

    if (result.status === 'error') {
      throw new Error(result.error)
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
    const result = await commands.clearStorage()

    if (result.status === 'error') {
      throw new Error(result.error)
    }
  },
}
