import { createContext, useContext } from 'react'
import type { FolderId, ProxyGroupLayout } from '@nyanpasu/rpc/types'

export type GroupLayoutActions = {
  /**
   * The arrangement of the groups the core reports now, or `undefined` while
   * they load. Every group appears exactly once.
   */
  layout: ProxyGroupLayout | undefined
  /** Whether the user arranged the groups of the active profile. */
  arranged: boolean
  /**
   * Commits `next` as one arrangement. Nothing is sent when it equals the
   * current one; a failure shows an error message. Resolves to whether it
   * committed.
   */
  commitLayout: (next: ProxyGroupLayout) => Promise<boolean>
  /** Moves a group into a folder (`null`: out of any folder), at `index`. */
  moveGroup: (
    name: string,
    destination: FolderId | null,
    index?: number,
  ) => Promise<boolean>
  /** Moves a group one place up or down inside its list or folder. */
  moveGroupBy: (name: string, offset: -1 | 1) => Promise<boolean>
  /** Creates a folder holding `members`, which leave the places they were in. */
  createFolderWith: (
    name: string,
    members?: string[],
  ) => Promise<FolderId | undefined>
  renameFolder: (id: FolderId, name: string) => Promise<boolean>
  /**
   * Deletes a folder; its groups move to "Groups". A folder with groups asks
   * first. Resolves to whether the folder was deleted.
   */
  deleteFolder: (id: FolderId) => Promise<boolean>
  /** Moves a folder one place up or down among the folders. */
  moveFolderBy: (id: FolderId, offset: -1 | 1) => Promise<boolean>
  /**
   * Removes the arrangement of the active profile, after asking: the groups
   * return to the core's order and the folders are deleted.
   */
  resetOrder: () => Promise<boolean>
  /** Opens the name dialog for a new folder, which then holds `members`. */
  openCreateFolder: (options?: {
    members?: string[]
    onCreated?: (id: FolderId) => void
  }) => void
  openRenameFolder: (id: FolderId) => void
}

export const GroupLayoutContext = createContext<GroupLayoutActions | null>(null)

export const useGroupLayout = () => {
  const context = useContext(GroupLayoutContext)

  if (!context) {
    throw new Error(
      'useGroupLayout must be used within a ProxiesLayoutProvider',
    )
  }

  return context
}
