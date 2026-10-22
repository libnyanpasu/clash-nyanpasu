import type { FolderId, ProfileId, ProfileLayout } from '@nyanpasu/rpc/types'

/**
 * Ids of the drag-and-drop targets and the pure rules that turn a drop into
 * a move. The prefixes keep the ids of sections, sidebar items and cards
 * apart, because a card's id is its profile uid.
 */

/** The `type` of a draggable profile card. */
export const PROFILE_DRAG_TYPE = 'profile'
/** The `type` of a draggable folder in the sidebar. */
export const FOLDER_DRAG_TYPE = 'folder'
/** The `type` of a section's drop target. */
export const SECTION_DRAG_TYPE = 'section'

/** The sections' key for "not in a folder". */
const UNGROUPED = 'ungrouped'

const SECTION_PREFIX = 'section:'
const SIDEBAR_FOLDER_PREFIX = 'folder:'
const COLLAPSED_SECTION_PREFIX = 'collapsed:'

/** The sidebar's "All profiles" item. */
export const NAV_ALL_ID = 'nav:all'

/** The profiles of each section, by section key. */
export type SectionItems = Record<string, ProfileId[]>

/** Where a dropped profile goes; `index` is its place, the end when omitted. */
export type ProfileDrop = {
  destination: FolderId | null
  index?: number
}

/** The id of a section's container, which is also its key in `SectionItems`. */
export const sectionId = (folder: FolderId | null) =>
  `${SECTION_PREFIX}${folder ?? UNGROUPED}`

/** A folder in the sidebar. */
export const sidebarFolderId = (folder: FolderId) =>
  `${SIDEBAR_FOLDER_PREFIX}${folder}`

/** A collapsed section's heading, which takes a drop but shows no cards. */
export const collapsedSectionId = (folder: FolderId | null) =>
  `${COLLAPSED_SECTION_PREFIX}${folder ?? UNGROUPED}`

export const folderOfSidebarId = (id: string): FolderId | undefined =>
  id.startsWith(SIDEBAR_FOLDER_PREFIX)
    ? id.slice(SIDEBAR_FOLDER_PREFIX.length)
    : undefined

/** The profiles of every section of `layout`, keyed by section id. */
export const sectionItemsOf = (layout: ProfileLayout): SectionItems => ({
  ...Object.fromEntries(
    layout.folders.map((folder) => [sectionId(folder.id), folder.members]),
  ),
  [sectionId(null)]: layout.ungrouped,
})

/**
 * `items` without `uid`: the profile the list shows apart from its sections,
 * so it takes no place in them.
 */
export const withoutProfile = (
  items: SectionItems,
  uid: ProfileId | null | undefined,
): SectionItems =>
  Object.fromEntries(
    Object.entries(items).map(([key, uids]) => [
      key,
      uids.filter((item) => item !== uid),
    ]),
  )

const destinationOfKey = (
  key: string,
  prefix: string,
): FolderId | null | undefined => {
  if (!key.startsWith(prefix)) return undefined

  const name = key.slice(prefix.length)

  return name === UNGROUPED ? null : name
}

/**
 * The destination of a target that takes a profile without showing it, such
 * as a sidebar item. `undefined` for a card or an open section.
 */
export const appendDestinationOf = (
  targetId: string,
): FolderId | null | undefined => {
  if (targetId === NAV_ALL_ID) return null

  return (
    folderOfSidebarId(targetId) ??
    destinationOfKey(targetId, COLLAPSED_SECTION_PREFIX)
  )
}

/**
 * Where a drop puts `uid`, or `null` when it moves nothing.
 *
 * A sidebar item or a collapsed heading appends. A card or an open section
 * leaves the profile where the drag preview `items` shows it, which is the
 * place the user saw it take. The preview may leave out profiles that the
 * `layout` keeps in the section, so its place is translated to the layout's:
 * the profile goes before the one that follows it in the preview.
 */
export const resolveProfileDrop = ({
  uid,
  targetId,
  items,
  layout,
}: {
  uid: ProfileId
  targetId: string | undefined
  items: SectionItems | null
  layout: ProfileLayout
}): ProfileDrop | null => {
  if (targetId === undefined) return null

  const destination = appendDestinationOf(targetId)

  if (destination !== undefined) return { destination }

  for (const [key, uids] of Object.entries(items ?? {})) {
    const index = uids.indexOf(uid)

    if (index === -1) continue

    const folder = destinationOfKey(key, SECTION_PREFIX) ?? null

    const members = (
      folder === null
        ? layout.ungrouped
        : (layout.folders.find((item) => item.id === folder)?.members ?? [])
    ).filter((member) => member !== uid)

    const next = members.indexOf(uids[index + 1])

    return {
      destination: folder,
      index: next === -1 ? members.length : next,
    }
  }

  return null
}
