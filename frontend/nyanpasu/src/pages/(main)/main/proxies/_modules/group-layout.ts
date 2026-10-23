import { isEqual } from 'es-toolkit'
import type {
  FolderId,
  ProxyGroupFolder,
  ProxyGroupLayout,
} from '@nyanpasu/rpc/types'

/**
 * Pure edits of a proxy group arrangement, made on the resolved layout (every
 * live group placed once). A destination of `null` is "not in a folder". Every
 * edit returns a new layout and leaves an unknown folder or group alone, so
 * the caller can compare the result with the input to tell a no-op.
 */

export const isSameGroupLayout = (a: ProxyGroupLayout, b: ProxyGroupLayout) =>
  isEqual(a, b)

/** Puts `name` at `index` of `destination` (the end when omitted). */
export const moveGroupInLayout = (
  layout: ProxyGroupLayout,
  name: string,
  destination: FolderId | null,
  index?: number,
): ProxyGroupLayout => {
  if (
    destination !== null &&
    !layout.folders.some((folder) => folder.id === destination)
  ) {
    return layout
  }

  const without = (names: string[]) => names.filter((item) => item !== name)
  const insert = (names: string[]) => {
    const next = without(names)
    next.splice(index ?? next.length, 0, name)
    return next
  }

  return {
    folders: layout.folders.map((folder) => ({
      ...folder,
      members:
        folder.id === destination
          ? insert(folder.members)
          : without(folder.members),
    })),
    ungrouped:
      destination === null
        ? insert(layout.ungrouped)
        : without(layout.ungrouped),
  }
}

/** Moves `name` one place up or down inside the list or folder it is in. */
export const moveGroupByInLayout = (
  layout: ProxyGroupLayout,
  name: string,
  offset: -1 | 1,
): ProxyGroupLayout => {
  const swap = (names: string[]) => {
    const index = names.indexOf(name)
    const target = index + offset

    if (index === -1 || target < 0 || target >= names.length) {
      return names
    }

    const next = [...names]
    ;[next[index], next[target]] = [next[target], next[index]]
    return next
  }

  return {
    folders: layout.folders.map((folder) => ({
      ...folder,
      members: swap(folder.members),
    })),
    ungrouped: swap(layout.ungrouped),
  }
}

export const renameGroupFolderInLayout = (
  layout: ProxyGroupLayout,
  id: FolderId,
  name: string,
): ProxyGroupLayout => ({
  ...layout,
  folders: layout.folders.map((folder) =>
    folder.id === id ? { ...folder, name } : folder,
  ),
})

/** Drops the folder; its groups move to the end of "Groups", in their order. */
export const removeGroupFolderFromLayout = (
  layout: ProxyGroupLayout,
  id: FolderId,
): ProxyGroupLayout => {
  const removed = layout.folders.find((folder) => folder.id === id)
  if (!removed) return layout

  return {
    folders: layout.folders.filter((folder) => folder.id !== id),
    ungrouped: [...layout.ungrouped, ...removed.members],
  }
}

/** Moves the folder one place up or down among the folders. */
export const moveGroupFolderByInLayout = (
  layout: ProxyGroupLayout,
  id: FolderId,
  offset: -1 | 1,
): ProxyGroupLayout => {
  const index = layout.folders.findIndex((folder) => folder.id === id)
  const target = index + offset

  if (index === -1 || target < 0 || target >= layout.folders.length) {
    return layout
  }

  const folders: ProxyGroupFolder[] = [...layout.folders]
  ;[folders[index], folders[target]] = [folders[target], folders[index]]

  return { ...layout, folders }
}

/** Where `name` sits: the folder it is in (`null`: the groups), and its place. */
export const locateGroup = (layout: ProxyGroupLayout, name: string) => {
  const folder = layout.folders.find((item) => item.members.includes(name))
  const names = folder ? folder.members : layout.ungrouped
  const index = names.indexOf(name)

  return index === -1
    ? undefined
    : { folder: folder ?? null, index, count: names.length }
}

/** Puts the folders in the order of `ids`; unknown ids are ignored. */
export const reorderGroupFoldersInLayout = (
  layout: ProxyGroupLayout,
  ids: readonly FolderId[],
): ProxyGroupLayout => {
  const byId = new Map(layout.folders.map((folder) => [folder.id, folder]))
  const ordered = ids.flatMap((id) => byId.get(id) ?? [])
  const rest = layout.folders.filter((folder) => !ids.includes(folder.id))

  return { ...layout, folders: [...ordered, ...rest] }
}
