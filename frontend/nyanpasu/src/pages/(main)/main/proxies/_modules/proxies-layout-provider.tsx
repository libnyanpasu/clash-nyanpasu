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
import {
  resolveProxyGroupLayout,
  storedProxyGroupLayoutOf,
  useClashProxies,
  useProfile,
} from '@nyanpasu/query'
import type { FolderId, ProxyGroupLayout } from '@nyanpasu/rpc/types'
import FolderNameDialog from '../../_modules/folder-name-dialog'
import {
  isSameGroupLayout,
  moveGroupByInLayout,
  moveGroupFolderByInLayout,
  moveGroupInLayout,
  removeGroupFolderFromLayout,
  renameGroupFolderInLayout,
} from './group-layout'
import { GroupLayoutContext, type GroupLayoutActions } from './use-group-layout'

type NameDialog =
  | {
      mode: 'create'
      members: string[]
      onCreated?: (id: FolderId) => void
    }
  | { mode: 'rename'; id: FolderId }

const showLayoutError = (error: unknown) =>
  message(m.proxies_layout_failed({ error: formatError(error) }), {
    title: m.proxies_layout_error_title(),
    kind: 'error',
    error,
  })

const EMPTY_LAYOUT: ProxyGroupLayout = { folders: [], ungrouped: [] }

/**
 * Owns every change to the proxy group arrangement: one commit per change, the
 * failure message and the folder name dialog. Descendants read it through
 * `useGroupLayout`.
 */
export default function ProxiesLayoutProvider({ children }: PropsWithChildren) {
  const { query, setProxyGroupLayout, createProxyGroupFolder } = useProfile()
  const {
    proxies: { data: proxies },
  } = useClashProxies()

  const profile = query.data?.current ?? undefined
  const stored = storedProxyGroupLayoutOf(query.data)

  const groups = proxies?.groups

  // The arrangement the sidebar shows: the stored one reconciled with the
  // groups the core reports now.
  const layout = useMemo(
    () =>
      groups
        ? resolveProxyGroupLayout(
            stored,
            groups.map((group) => group.name),
          )
        : undefined,
    [groups, stored],
  )

  // Callbacks read the latest values through refs, so they stay stable and a
  // change made right after another sees that one's optimistic result.
  const latest = useRef({ layout, profile })
  latest.current = { layout, profile }

  const [dialog, setDialog] = useState<NameDialog | null>(null)

  const { mutateAsync: setLayoutAsync } = setProxyGroupLayout
  const { mutateAsync: createFolderAsync } = createProxyGroupFolder

  const commitTo = useCallback(
    async (target: string, next: ProxyGroupLayout) => {
      try {
        await setLayoutAsync({ profile: target, layout: next })
      } catch (error) {
        showLayoutError(error)

        return false
      }

      return true
    },
    [setLayoutAsync],
  )

  const commitLayout = useCallback(
    async (next: ProxyGroupLayout) => {
      const { layout: previous, profile: target } = latest.current

      if (!previous || !target || isSameGroupLayout(previous, next)) {
        return false
      }

      return commitTo(target, next)
    },
    [commitTo],
  )

  const edit = useCallback(
    (change: (current: ProxyGroupLayout) => ProxyGroupLayout) => {
      const current = latest.current.layout

      return current ? commitLayout(change(current)) : Promise.resolve(false)
    },
    [commitLayout],
  )

  const moveGroup = useCallback<GroupLayoutActions['moveGroup']>(
    (name, destination, index) =>
      edit((current) => moveGroupInLayout(current, name, destination, index)),
    [edit],
  )

  const moveGroupBy = useCallback<GroupLayoutActions['moveGroupBy']>(
    (name, offset) =>
      edit((current) => moveGroupByInLayout(current, name, offset)),
    [edit],
  )

  const createFolderWith = useCallback<GroupLayoutActions['createFolderWith']>(
    async (name, members = []) => {
      const target = latest.current.profile

      if (!target) {
        return undefined
      }

      try {
        const outcome = await createFolderAsync({
          profile: target,
          name,
          members,
        })

        return outcome.value
      } catch (error) {
        showLayoutError(error)

        return undefined
      }
    },
    [createFolderAsync],
  )

  const renameFolder = useCallback<GroupLayoutActions['renameFolder']>(
    (id, name) =>
      edit((current) => renameGroupFolderInLayout(current, id, name)),
    [edit],
  )

  const deleteFolder = useCallback<GroupLayoutActions['deleteFolder']>(
    async (id) => {
      const folder = latest.current.layout?.folders.find(
        (item) => item.id === id,
      )

      if (!folder) {
        return false
      }

      if (folder.members.length > 0) {
        const confirmed = await ask(
          m.proxies_folder_delete_description({ count: folder.members.length }),
          { title: m.profile_folder_delete_title(), kind: 'warning' },
        )

        if (!confirmed) {
          return false
        }
      }

      // The arrangement may have changed while the dialog was open.
      return edit((current) => removeGroupFolderFromLayout(current, id))
    },
    [edit],
  )

  const moveFolderBy = useCallback<GroupLayoutActions['moveFolderBy']>(
    (id, offset) =>
      edit((current) => moveGroupFolderByInLayout(current, id, offset)),
    [edit],
  )

  const resetOrder = useCallback<GroupLayoutActions['resetOrder']>(async () => {
    const target = latest.current.profile

    if (!target) {
      return false
    }

    const confirmed = await ask(m.proxies_nav_reset_order_description(), {
      title: m.proxies_nav_reset_order(),
      kind: 'warning',
    })

    return confirmed ? commitTo(target, EMPTY_LAYOUT) : false
  }, [commitTo])

  const openCreateFolder = useCallback<GroupLayoutActions['openCreateFolder']>(
    (options) =>
      setDialog({
        mode: 'create',
        members: options?.members ?? [],
        onCreated: options?.onCreated,
      }),
    [],
  )

  const openRenameFolder = useCallback<GroupLayoutActions['openRenameFolder']>(
    (id) => setDialog({ mode: 'rename', id }),
    [],
  )

  const arranged = stored !== undefined

  const value = useMemo<GroupLayoutActions>(
    () => ({
      layout,
      arranged,
      commitLayout,
      moveGroup,
      moveGroupBy,
      createFolderWith,
      renameFolder,
      deleteFolder,
      moveFolderBy,
      resetOrder,
      openCreateFolder,
      openRenameFolder,
    }),
    [
      layout,
      arranged,
      commitLayout,
      moveGroup,
      moveGroupBy,
      createFolderWith,
      renameFolder,
      deleteFolder,
      moveFolderBy,
      resetOrder,
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
    <GroupLayoutContext.Provider value={value}>
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
    </GroupLayoutContext.Provider>
  )
}
