import {
  THEME_CSS_VARS_KEY,
  THEME_PALETTE_KEY,
} from '@/components/providers/theme-provider'
import { isCustomCssStyleId } from '@/utils/custom-css-items'
import { CACHED_LANGUAGE_STORAGE_KEY } from '@/utils/language'
import { clearKvStorageLocalCache } from '@nyanpasu/query'
import type { QueryClient } from '@tanstack/react-query'
import type { RpcDescribeSnapshot } from './capabilities'

/** Records which principal and scopes the browser's persisted state belongs to. */
export const PRINCIPAL_MARKER_KEY = 'nyanpasu-principal-marker'

/**
 * The non-secret identity of a page runtime: principal kind and id plus the
 * sorted scope set. The desktop webview is one caller whatever its window
 * label, so its id is dropped.
 */
export function principalKey({
  principal,
  scopes,
}: Pick<RpcDescribeSnapshot, 'principal' | 'scopes'>): string {
  const id = principal.kind === 'desktop' ? 'desktop' : principal.id
  return JSON.stringify([principal.kind, id, [...scopes].sort()])
}

export function readPrincipalMarker(): string | null {
  try {
    return localStorage.getItem(PRINCIPAL_MARKER_KEY)
  } catch {
    return null
  }
}

export function clearPrincipalMarker() {
  try {
    localStorage.removeItem(PRINCIPAL_MARKER_KEY)
  } catch {
    // ignore security errors
  }
}

/**
 * Drops everything one principal left behind in this browser: persisted KV
 * values, cached theme and language, injected custom CSS, and cached queries.
 * Synchronous, so it can finish before any provider mounts.
 */
export function clearPrincipalState(queryClient: QueryClient) {
  clearKvStorageLocalCache()
  try {
    for (const key of [
      THEME_PALETTE_KEY,
      THEME_CSS_VARS_KEY,
      CACHED_LANGUAGE_STORAGE_KEY,
    ]) {
      localStorage.removeItem(key)
    }
  } catch {
    // ignore security errors
  }
  document.head
    .querySelectorAll<HTMLStyleElement>('style[id]')
    .forEach((element) => {
      if (isCustomCssStyleId(element.id)) element.remove()
    })
  queryClient.clear()
}

/** Clears a revoked page runtime's state, including its marker. */
export function clearRevokedPrincipalState(queryClient: QueryClient) {
  clearPrincipalState(queryClient)
  clearPrincipalMarker()
}

/**
 * Binds this browser's persisted state to `snapshot`'s principal. When the
 * stored marker names another principal or scope set, or none, the state is
 * cleared first. Returns the marker now stored.
 */
export function reconcilePrincipalMarker(
  snapshot: RpcDescribeSnapshot,
  queryClient: QueryClient,
): string {
  const key = principalKey(snapshot)
  if (readPrincipalMarker() !== key) {
    clearPrincipalState(queryClient)
    try {
      localStorage.setItem(PRINCIPAL_MARKER_KEY, key)
    } catch {
      // ignore quota / security errors
    }
  }
  return key
}
