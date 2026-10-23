import type { FolderId, ProxyGroupLayout } from '@nyanpasu/rpc/types'

/**
 * Ids of the drag-and-drop targets and the pure rule that turns a drop into a
 * move. The prefixes keep the ids of groups, folders, folder headings and
 * sections apart, because a group's id would otherwise be its name.
 */

/** The `type` of a draggable group item. */
export const GROUP_DRAG_TYPE = 'group'
/** The `type` of a draggable folder. */
export const FOLDER_DRAG_TYPE = 'folder'
/** The `type` of a folder heading, which takes a group dropped on it. */
export const HEADING_DRAG_TYPE = 'heading'
/** The `type` of the list of a section, which takes a group into an empty one. */
export const SECTION_DRAG_TYPE = 'section'

/** The sortable group the folders move in. */
export const FOLDERS_DRAG_GROUP = 'folders'

/** The sections' key for "not in a folder". */
const UNGROUPED = 'ungrouped'

const GROUP_PREFIX = 'group:'
const FOLDER_PREFIX = 'folder:'
const HEADING_PREFIX = 'heading:'
const SECTION_PREFIX = 'section:'

/** The groups of each section, as drag ids, by section id. */
export type SectionItems = Record<string, string[]>

/** Where a dropped group goes; `index` is its place, the end when omitted. */
export type GroupDrop = {
  destination: FolderId | null
  index?: number
}

export const groupDragId = (name: string) => `${GROUP_PREFIX}${name}`

export const groupOfDragId = (id: string): string | undefined =>
  id.startsWith(GROUP_PREFIX) ? id.slice(GROUP_PREFIX.length) : undefined

export const folderDragId = (folder: FolderId) => `${FOLDER_PREFIX}${folder}`

export const folderOfDragId = (id: string): FolderId | undefined =>
  id.startsWith(FOLDER_PREFIX) ? id.slice(FOLDER_PREFIX.length) : undefined

export const folderHeadingId = (folder: FolderId) =>
  `${HEADING_PREFIX}${folder}`

export const folderOfHeadingId = (id: string): FolderId | undefined =>
  id.startsWith(HEADING_PREFIX) ? id.slice(HEADING_PREFIX.length) : undefined

/** The id of a section's list, which is also its key in `SectionItems`. */
export const sectionId = (folder: FolderId | null) =>
  `${SECTION_PREFIX}${folder ?? UNGROUPED}`

const folderOfSectionId = (id: string): FolderId | null | undefined => {
  if (!id.startsWith(SECTION_PREFIX)) return undefined

  const key = id.slice(SECTION_PREFIX.length)

  return key === UNGROUPED ? null : key
}

/** The groups of every section of `layout`, keyed by section id. */
export const sectionItemsOf = (layout: ProxyGroupLayout): SectionItems => ({
  ...Object.fromEntries(
    layout.folders.map((folder) => [
      sectionId(folder.id),
      folder.members.map(groupDragId),
    ]),
  ),
  [sectionId(null)]: layout.ungrouped.map(groupDragId),
})

/** The folders of `layout`, as drag ids, in order. */
export const folderDragIdsOf = (layout: ProxyGroupLayout) =>
  layout.folders.map((folder) => folderDragId(folder.id))

/**
 * Where a drop puts the group `name`, or `null` when it moves nothing.
 *
 * A folder heading appends, whether the folder is open or closed. Anything
 * else leaves the group where the drag preview `items` shows it, which is the
 * place the user saw it take. The preview lists every group of its section, so
 * its place is the place in the layout.
 */
export const resolveGroupDrop = ({
  name,
  targetId,
  items,
}: {
  name: string
  targetId: string | undefined
  items: SectionItems | null
}): GroupDrop | null => {
  if (targetId === undefined) return null

  const heading = folderOfHeadingId(targetId)

  if (heading !== undefined) return { destination: heading }

  const id = groupDragId(name)

  for (const [key, ids] of Object.entries(items ?? {})) {
    const index = ids.indexOf(id)

    if (index === -1) continue

    const destination = folderOfSectionId(key)

    return destination === undefined ? null : { destination, index }
  }

  return null
}

/** The group names behind a list of group drag ids. */
export const groupNamesOf = (ids: readonly string[]) =>
  ids.flatMap((id) => groupOfDragId(id) ?? [])
