const CLIENT_ID_STORAGE_KEY = 'nyanpasu-client-id'

// Identifies this tab when `sessionStorage` is unavailable.
let memoryClientId: string | null = null

/**
 * The opaque per-tab identifier sent as `X-Nyanpasu-Client-Id`, so that
 * concurrent tabs of one principal own independent resources. It lives in
 * `sessionStorage`: a tab keeps it across reloads, and tabs never share it.
 */
export function getClientId(): string {
  try {
    const stored = sessionStorage.getItem(CLIENT_ID_STORAGE_KEY)
    if (stored) return stored
    const created = crypto.randomUUID()
    sessionStorage.setItem(CLIENT_ID_STORAGE_KEY, created)
    return created
  } catch {
    // Non-DOM and private contexts: stay stable for this runtime only.
    memoryClientId ??= crypto.randomUUID()
    return memoryClientId
  }
}

/** `EventSource` cannot send headers, so streams carry the id in the URL. */
export function withClientId(path: string): string {
  const separator = path.includes('?') ? '&' : '?'
  return `${path}${separator}client_id=${encodeURIComponent(getClientId())}`
}
