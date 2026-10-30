import {
  PropsWithChildren,
  useCallback,
  useMemo,
  useRef,
  useState,
} from 'react'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { ask, message } from '@/utils/notification'
import { profileLayoutOf, useProfile } from '@nyanpasu/query'
import type { FolderId, ProfileId, ProfileLayout } from '@nyanpasu/rpc/types'
import FolderNameDialog from '../../_modules/folder-name-dialog'
import {
  isSameLayout,
  moveProfileInLayout,
  removeFolderFromLayout,
  renameFolderInLayout,
  reorderFoldersInLayout,
} from './profile-layout'
import {
  ProfilesLayoutContext,
  type ProfileLayoutActions,
} from './use-profile-layout'

type NameDialog =
  | {
      mode: 'create'
      members: ProfileId[]
      onCreated?: (id: FolderId) => void
    }
  | { mode: 'rename'; id: FolderId }

const showLayoutError = (error: unknown) =>
  message(m.profile_folder_layout_failed({ error: formatError(error) }), {
    title: m.profile_error_title(),
    kind: 'error',
    error,
  })

/**
 * Owns every change to the profile folders and their order: one commit per
 * change, the failure message and the folder name dialog. Descendants read it
 * through `useProfileLayout`.
 */
export default function ProfilesLayoutProvider({
  children,
}: PropsWithChildren) {
  const { query, setLayout, createFolder } = useProfile()

  const profiles = query.data

  const layout = useMemo(
    () => (profiles ? profileLayoutOf(profiles) : undefined),
    [profiles],
  )

  // Callbacks read the latest document through refs, so they stay stable and
  // a drop made right after another sees that one's optimistic result.
  const layoutRef = useRef(layout)
  layoutRef.current = layout

  const [dialog, setDialog] = useState<NameDialog | null>(null)

  const { mutateAsync: setLayoutAsync } = setLayout
  const { mutateAsync: createFolderAsync } = createFolder

  const commitLayout = useCallback(
    async (next: ProfileLayout) => {
      const previous = layoutRef.current

      if (!previous || isSameLayout(previous, next)) {
        return false
      }

      try {
        await setLayoutAsync(next)
      } catch (error) {
        showLayoutError(error)

        return false
      }

      return true
    },
    [setLayoutAsync],
  )

  const moveProfile = useCallback<ProfileLayoutActions['moveProfile']>(
    (uid, destination, index) => {
      const current = layoutRef.current

      return current
        ? commitLayout(moveProfileInLayout(current, uid, destination, index))
        : Promise.resolve(false)
    },
    [commitLayout],
  )

  const createFolderWith = useCallback<
    ProfileLayoutActions['createFolderWith']
  >(
    async (name, members = []) => {
      try {
        const outcome = await createFolderAsync({ name, members })

        return outcome.value
      } catch (error) {
        showLayoutError(error)

        return undefined
      }
    },
    [createFolderAsync],
  )

  const renameFolder = useCallback<ProfileLayoutActions['renameFolder']>(
    (id, name) => {
      const current = layoutRef.current

      return current
        ? commitLayout(renameFolderInLayout(current, id, name))
        : Promise.resolve(false)
    },
    [commitLayout],
  )

  const deleteFolder = useCallback<ProfileLayoutActions['deleteFolder']>(
    async (id) => {
      const current = layoutRef.current
      const folder = current?.folders.find((item) => item.id === id)

      if (!current || !folder) {
        return false
      }

      if (folder.members.length > 0) {
        const confirmed = await ask(
          m.profile_folder_delete_description({ count: folder.members.length }),
          { title: m.profile_folder_delete_title(), kind: 'warning' },
        )

        if (!confirmed) {
          return false
        }
      }

      // The document may have changed while the dialog was open.
      const latest = layoutRef.current

      return latest ? commitLayout(removeFolderFromLayout(latest, id)) : false
    },
    [commitLayout],
  )

  const reorderFolders = useCallback<ProfileLayoutActions['reorderFolders']>(
    (ids) => {
      const current = layoutRef.current

      return current
        ? commitLayout(reorderFoldersInLayout(current, ids))
        : Promise.resolve(false)
    },
    [commitLayout],
  )

  const openCreateFolder = useCallback<
    ProfileLayoutActions['openCreateFolder']
  >(
    (options) =>
      setDialog({
        mode: 'create',
        members: options?.members ?? [],
        onCreated: options?.onCreated,
      }),
    [],
  )

  const openRenameFolder = useCallback<
    ProfileLayoutActions['openRenameFolder']
  >((id) => setDialog({ mode: 'rename', id }), [])

  const value = useMemo<ProfileLayoutActions>(
    () => ({
      layout,
      commitLayout,
      moveProfile,
      createFolderWith,
      renameFolder,
      deleteFolder,
      reorderFolders,
      openCreateFolder,
      openRenameFolder,
    }),
    [
      layout,
      commitLayout,
      moveProfile,
      createFolderWith,
      renameFolder,
      deleteFolder,
      reorderFolders,
      openCreateFolder,
      openRenameFolder,
    ],
  )

  const renaming =
    dialog?.mode === 'rename'
      ? layout?.folders.find((folder) => folder.id === dialog.id)
      : undefined

  const handleSubmitName = async (name: string) => {
    if (!dialog) {
      return
    }

    if (dialog.mode === 'rename') {
      // An unchanged name commits nothing, which is not a failure.
      if (name === renaming?.name || (await renameFolder(dialog.id, name))) {
        setDialog(null)
      }

      return
    }

    const id = await createFolderWith(name, dialog.members)

    if (id) {
      setDialog(null)
      dialog.onCreated?.(id)
    }
  }

  return (
    <ProfilesLayoutContext.Provider value={value}>
      {children}

      <FolderNameDialog
        open={dialog !== null && (dialog.mode === 'create' || !!renaming)}
        onOpenChange={(open) => !open && setDialog(null)}
        title={
          dialog?.mode === 'rename'
            ? m.profile_folder_rename_title()
            : m.profile_folder_new_title()
        }
        initialName={renaming?.name}
        onSubmit={handleSubmitName}
      />
    </ProfilesLayoutContext.Provider>
  )
}
