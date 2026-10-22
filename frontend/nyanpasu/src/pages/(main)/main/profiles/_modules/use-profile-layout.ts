import { createContext, useContext } from 'react'
import type { FolderId, ProfileId, ProfileLayout } from '@nyanpasu/rpc/types'

export type ProfileLayoutActions = {
  /** The layout the profiles document has now, or `undefined` while loading. */
  layout: ProfileLayout | undefined
  /**
   * Commits `next` as one layout. Nothing is sent when it equals the current
   * layout; a failure shows an error message. Resolves to whether it committed.
   */
  commitLayout: (next: ProfileLayout) => Promise<boolean>
  /** Moves a profile into a folder (`null`: out of any folder), at `index`. */
  moveProfile: (
    uid: ProfileId,
    destination: FolderId | null,
    index?: number,
  ) => Promise<boolean>
  /** Creates a folder holding `members`, which leave the folders they were in. */
  createFolderWith: (
    name: string,
    members?: ProfileId[],
  ) => Promise<FolderId | undefined>
  renameFolder: (id: FolderId, name: string) => Promise<boolean>
  /**
   * Deletes a folder; its profiles move to "not in a folder". A folder with
   * profiles asks first. Resolves to whether the folder was deleted.
   */
  deleteFolder: (id: FolderId) => Promise<boolean>
  /** Sets the folder order. */
  reorderFolders: (ids: readonly FolderId[]) => Promise<boolean>
  /** Opens the name dialog for a new folder, which then holds `members`. */
  openCreateFolder: (options?: {
    members?: ProfileId[]
    onCreated?: (id: FolderId) => void
  }) => void
  openRenameFolder: (id: FolderId) => void
}

export const ProfilesLayoutContext = createContext<ProfileLayoutActions | null>(
  null,
)

export const useProfileLayout = () => {
  const context = useContext(ProfilesLayoutContext)

  if (!context) {
    throw new Error(
      'useProfileLayout must be used within a ProfilesLayoutProvider',
    )
  }

  return context
}
