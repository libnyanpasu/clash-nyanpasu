/** Mirrors the `WEB_STORAGE_KEY_PREFIX` constant on the backend. */
const WEB_KEY_PREFIX = 'web:'

/** A committed value: its JSON text, or `null` once removed. */
export type KvStorageSnapshot = {
  value: string | null
  revision: number
}

export type KvStorageEntry = {
  /** The committed JSON text; `undefined` until a snapshot arrives. */
  value: string | null | undefined
  /** The revision of `value`; `null` until a snapshot arrives. */
  revision: number | null
  /** The newest local write that the backend has not acknowledged. */
  draft: { id: number; value: string; failed: boolean } | null
  read: { status: 'idle' | 'reading' | 'done'; error: unknown | null }
}

export const EMPTY_KV_STORAGE_ENTRY: KvStorageEntry = {
  value: undefined,
  revision: null,
  draft: null,
  read: { status: 'idle', error: null },
}

export const kvStorageQueryKey = (key: string) => ['kvStorage', key] as const

// Reads, events and write acknowledgements all pass through here, so an older
// snapshot never replaces a newer one whatever order they arrive in. A failed
// draft gives way to a newer commit; a pending one waits for its own answer.
export function mergeKvStorageSnapshot(
  entry: KvStorageEntry,
  snapshot: KvStorageSnapshot,
): KvStorageEntry {
  if (entry.revision !== null && snapshot.revision <= entry.revision) {
    return entry
  }
  return {
    ...entry,
    value: snapshot.value,
    revision: snapshot.revision,
    draft: entry.draft?.failed ? null : entry.draft,
  }
}

/**
 * Turns a `StorageValueChangedEvent` into the snapshot of a frontend key, or
 * `null` for internal keys and malformed values. The event carries the stored
 * bytes, which are the JSON encoding of the value's JSON text.
 */
export function kvStorageEventSnapshot(event: {
  key: string
  value: string | null
  revision: number
}): { key: string; snapshot: KvStorageSnapshot } | null {
  if (!event.key.startsWith(WEB_KEY_PREFIX)) return null
  const key = event.key.slice(WEB_KEY_PREFIX.length)
  if (event.value === null) {
    return { key, snapshot: { value: null, revision: event.revision } }
  }
  try {
    const value: unknown = JSON.parse(event.value)
    if (typeof value !== 'string') return null
    JSON.parse(value)
    return { key, snapshot: { value, revision: event.revision } }
  } catch {
    return null
  }
}
