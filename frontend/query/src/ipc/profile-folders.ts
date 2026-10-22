import type {
  ProfileFolder,
  ProfileId,
  ProfileItem_Serialize,
  ProfileLayout,
} from '@nyanpasu/rpc/types'

type ConfigItem = Extract<ProfileItem_Serialize, { type: 'config' }>

/** The part of the profiles document that folders and their order depend on. */
export type ProfileFoldersSource = {
  items: ProfileItem_Serialize[]
  folders?: ProfileFolder[]
}

export type ProfileFolderSection = {
  folder: ProfileFolder
  profiles: ConfigItem[]
}

export type ProfileSections = {
  /** Folders in their order, each with its Config profiles in member order. */
  folders: ProfileFolderSection[]
  /** The Config profiles that are in no folder, in document order. */
  ungrouped: ConfigItem[]
}

/** Splits the Config profiles into the folders that hold them and the rest. */
export function profileSections(source: ProfileFoldersSource): ProfileSections {
  const configs = new Map<ProfileId, ConfigItem>()
  for (const item of source.items) {
    if (item.type === 'config') configs.set(item.uid, item)
  }

  const placed = new Set<ProfileId>()
  const folders = (source.folders ?? []).map((folder) => {
    const profiles: ConfigItem[] = []
    for (const uid of folder.members) {
      const item = configs.get(uid)
      if (!item) continue
      profiles.push(item)
      placed.add(uid)
    }
    return { folder, profiles }
  })

  const ungrouped = [...configs.values()].filter(
    (item) => !placed.has(item.uid),
  )

  return { folders, ungrouped }
}

/** The layout the document currently has. */
export function profileLayoutOf(source: ProfileFoldersSource): ProfileLayout {
  const { folders, ungrouped } = profileSections(source)

  return {
    folders: folders.map(({ folder, profiles }) => ({
      ...folder,
      members: profiles.map((item) => item.uid),
    })),
    ungrouped: ungrouped.map((item) => item.uid),
  }
}

/**
 * Applies a layout the way the backend commit does: the folders are replaced
 * and `items` is reordered to the folders' members, then the ungrouped Config
 * profiles, then everything else (the transforms) in its existing order.
 * It does not validate; the backend still rejects an invalid layout.
 */
export function applyProfileLayout<T extends ProfileFoldersSource>(
  source: T,
  layout: ProfileLayout,
): T {
  const previous = new Map(source.items.map((item) => [item.uid, item]))
  const items: ProfileItem_Serialize[] = []
  const ordered = [
    ...layout.folders.flatMap((folder) => folder.members),
    ...layout.ungrouped,
  ]

  for (const uid of ordered) {
    const item = previous.get(uid)
    if (!item) continue
    items.push(item)
    previous.delete(uid)
  }
  items.push(...previous.values())

  return { ...source, folders: layout.folders, items }
}
