import AddRounded from '~icons/material-symbols/add-rounded'
import CheckRounded from '~icons/material-symbols/check-rounded'
import DriveFileMoveOutlineRounded from '~icons/material-symbols/drive-file-move-outline-rounded'
import FolderOffOutlineRounded from '~icons/material-symbols/folder-off-outline-rounded'
import FolderRounded from '~icons/material-symbols/folder-rounded'
import {
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
} from '@nyanpasu/ui/context-menu'
import { m } from '@/paraglide/messages'
import type { FolderId, ProxyGroupFolder } from '@nyanpasu/rpc/types'
import { locateGroup } from './group-layout'
import { useGroupLayout } from './use-group-layout'

/**
 * What a "Move to folder" menu needs. It is read where the group is shown,
 * because a context menu renders outside the layout provider.
 */
export type GroupMove = {
  folders: ProxyGroupFolder[]
  /** The folder the group is in. */
  folder: ProxyGroupFolder | null
  /** Moves the group into `destination`, or out of its folder for `null`. */
  handleMove: (destination: FolderId | null) => void
  /** Asks for a name, then moves the group into the new folder. */
  handleCreate: () => void
}

export const useGroupMove = (name: string): GroupMove => {
  const { layout, moveGroup, openCreateFolder } = useGroupLayout()

  return {
    folders: layout?.folders ?? [],
    folder: (layout && locateGroup(layout, name)?.folder) ?? null,
    handleMove: (destination) => {
      moveGroup(name, destination)
    },
    handleCreate: () => openCreateFolder({ members: [name] }),
  }
}

/** "Move to folder" as a submenu of a context menu. */
export function GroupMoveContextMenu({ move }: { move: GroupMove }) {
  const { folders, folder, handleMove, handleCreate } = move

  return (
    <ContextMenuSub>
      <ContextMenuSubTrigger>
        <span className="flex items-center gap-2">
          <DriveFileMoveOutlineRounded className="size-4" />
          {m.profile_move_to_folder()}
        </span>
      </ContextMenuSubTrigger>

      <ContextMenuSubContent>
        {folders.map((item) => (
          <ContextMenuItem
            key={item.id}
            disabled={item.id === folder?.id}
            onSelect={() => handleMove(item.id)}
          >
            {item.id === folder?.id ? (
              <CheckRounded className="size-4" />
            ) : (
              <FolderRounded className="size-4" />
            )}
            <span className="truncate">{item.name}</span>
          </ContextMenuItem>
        ))}

        {folders.length > 0 && <ContextMenuSeparator />}

        <ContextMenuItem onSelect={handleCreate}>
          <AddRounded className="size-4" />
          <span className="truncate">{m.profile_move_new_folder()}</span>
        </ContextMenuItem>

        {folder && (
          <ContextMenuItem onSelect={() => handleMove(null)}>
            <FolderOffOutlineRounded className="size-4" />
            <span className="truncate">
              {m.profile_move_remove_from_folder()}
            </span>
          </ContextMenuItem>
        )}
      </ContextMenuSubContent>
    </ContextMenuSub>
  )
}
