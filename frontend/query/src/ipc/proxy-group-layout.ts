import type { ProfileId, ProxyGroupLayout } from '@nyanpasu/rpc/types'

/** The part of the profiles document that the group arrangement lives in. */
export type ProxyGroupLayoutSource = {
  current?: ProfileId | null
  proxy_group_layouts?: { [key in ProfileId]: ProxyGroupLayout }
}

/** The arrangement stored for the active profile, if the user made one. */
export function storedProxyGroupLayoutOf(
  source: ProxyGroupLayoutSource | undefined,
): ProxyGroupLayout | undefined {
  const current = source?.current

  return current ? source?.proxy_group_layouts?.[current] : undefined
}

/**
 * Reconciles a stored arrangement with the groups the core reports, in its
 * order. The Rust `ProxyGroupLayout::resolve` is the same function and the two
 * test suites carry the same cases.
 *
 * - A stored name that is not live is ignored, not forgotten.
 * - A live name the layout does not place is appended to the ungrouped groups,
 *   in the core's order.
 * - A name placed twice keeps its first place, ungrouped before folders.
 */
export function resolveProxyGroupLayout(
  layout: ProxyGroupLayout | undefined,
  live: readonly string[],
): ProxyGroupLayout {
  const liveNames = new Set(live)
  const placed = new Set<string>()
  const place = (name: string) => {
    if (!liveNames.has(name) || placed.has(name)) return false
    placed.add(name)
    return true
  }

  const ungrouped = (layout?.ungrouped ?? []).filter(place)
  const folders = (layout?.folders ?? []).map((folder) => ({
    ...folder,
    members: folder.members.filter(place),
  }))

  for (const name of live) {
    if (place(name)) ungrouped.push(name)
  }

  return { folders, ungrouped }
}

/** The groups in sidebar order, top to bottom: the ungrouped, then each folder. */
export function flatProxyGroupOrder(layout: ProxyGroupLayout): string[] {
  return [
    ...layout.ungrouped,
    ...layout.folders.flatMap((folder) => folder.members),
  ]
}

/** Whether the layout places nothing, which is the same as having none. */
export const isEmptyProxyGroupLayout = (layout: ProxyGroupLayout) =>
  layout.folders.length === 0 && layout.ungrouped.length === 0

/**
 * Applies a layout the way the backend commit does: the profile's entry is
 * replaced, or removed when the layout places nothing.
 */
export function applyProxyGroupLayout<T extends ProxyGroupLayoutSource>(
  source: T,
  profile: ProfileId,
  layout: ProxyGroupLayout,
): T & Required<Pick<ProxyGroupLayoutSource, 'proxy_group_layouts'>> {
  const { [profile]: _previous, ...rest } = source.proxy_group_layouts ?? {}

  return {
    ...source,
    proxy_group_layouts: isEmptyProxyGroupLayout(layout)
      ? rest
      : { ...rest, [profile]: layout },
  }
}
