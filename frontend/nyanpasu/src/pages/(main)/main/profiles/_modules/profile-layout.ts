import { isEqual } from 'es-toolkit'
import type {
  FolderId,
  ProfileFolder,
  ProfileId,
  ProfileLayout,
} from '@nyanpasu/rpc/types'

/**
 * Pure edits of a profile layout. A destination of `null` is "not in a
 * folder". Every edit returns a new layout and leaves an unknown folder or
 * profile alone, so the caller can compare the result with the input to tell
 * a no-op.
 */

export const isSameLayout = (a: ProfileLayout, b: ProfileLayout) =>
  isEqual(a, b)

/** Puts `uid` at `index` of `destination` (the end when omitted). */
export const moveProfileInLayout = (
  layout: ProfileLayout,
  uid: ProfileId,
  destination: FolderId | null,
  index?: number,
): ProfileLayout => {
  if (
    destination !== null &&
    !layout.folders.some((folder) => folder.id === destination)
  ) {
    return layout
  }

  const without = (uids: ProfileId[]) => uids.filter((item) => item !== uid)
  const insert = (uids: ProfileId[]) => {
    const next = without(uids)
    next.splice(index ?? next.length, 0, uid)
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

export const renameFolderInLayout = (
  layout: ProfileLayout,
  id: FolderId,
  name: string,
): ProfileLayout => ({
  ...layout,
  folders: layout.folders.map((folder) =>
    folder.id === id ? { ...folder, name } : folder,
  ),
})

/** Drops the folder; its profiles move to the end of "not in a folder". */
export const removeFolderFromLayout = (
  layout: ProfileLayout,
  id: FolderId,
): ProfileLayout => {
  const removed = layout.folders.find((folder) => folder.id === id)
  if (!removed) return layout

  return {
    folders: layout.folders.filter((folder) => folder.id !== id),
    ungrouped: [...layout.ungrouped, ...removed.members],
  }
}

/** Orders the folders as `ids`; a folder not listed keeps its place at the end. */
export const reorderFoldersInLayout = (
  layout: ProfileLayout,
  ids: readonly FolderId[],
): ProfileLayout => {
  const byId = new Map<FolderId, ProfileFolder>(
    layout.folders.map((folder) => [folder.id, folder]),
  )
  const folders: ProfileFolder[] = []

  for (const id of ids) {
    const folder = byId.get(id)
    if (!folder) continue
    folders.push(folder)
    byId.delete(id)
  }
  folders.push(...byId.values())

  return { ...layout, folders }
}
