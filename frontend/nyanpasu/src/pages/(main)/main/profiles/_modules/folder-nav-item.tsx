import ArrowDownwardRounded from '~icons/material-symbols/arrow-downward-rounded'
import ArrowUpwardRounded from '~icons/material-symbols/arrow-upward-rounded'
import DeleteForeverOutlineRounded from '~icons/material-symbols/delete-forever-outline-rounded'
import EditSquareOutlineRounded from '~icons/material-symbols/edit-square-outline-rounded'
import FolderRounded from '~icons/material-symbols/folder-rounded'
import { ContextMenuItem } from '@nyanpasu/ui/context-menu'
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider'
import { m } from '@/paraglide/messages'
import { FOLDER_ITEM_DRAG_SENSORS } from '@/utils/dnd-sensors'
import { useSortable } from '@dnd-kit/react/sortable'
import type { ProfileFolder } from '@nyanpasu/rpc/types'
import ProfilesNavigationItem from './profiles-navigation-item'
import {
  FOLDER_DRAG_TYPE,
  PROFILE_DRAG_TYPE,
  sidebarFolderId,
} from './resolve-profile-drop'
import { useProfileLayout } from './use-profile-layout'

const FOLDERS_GROUP = 'folders'

/**
 * A folder in the sidebar: a link to its page, with a menu to manage it. It
 * can be dragged to reorder the folders and takes a profile card dropped on it.
 */
export default function FolderNavItem({
  folder,
  index,
  count,
}: {
  folder: ProfileFolder
  /** Where the folder sits among all folders. */
  index: number
  /** How many folders there are. */
  count: number
}) {
  const { layout, openRenameFolder, deleteFolder, reorderFolders } =
    useProfileLayout()

  const { ref, isDragging, isDropTarget } = useSortable({
    id: sidebarFolderId(folder.id),
    index,
    group: FOLDERS_GROUP,
    type: FOLDER_DRAG_TYPE,
    accept: [FOLDER_DRAG_TYPE, PROFILE_DRAG_TYPE],
    sensors: FOLDER_ITEM_DRAG_SENSORS,
  })

  const moveBy = (offset: -1 | 1) => {
    const ids = (layout?.folders ?? []).map((item) => item.id)

    ;[ids[index], ids[index + offset]] = [ids[index + offset], ids[index]]

    return reorderFolders(ids)
  }

  return (
    <RegisterContextMenu>
      <RegisterContextMenuTrigger asChild>
        <ProfilesNavigationItem
          ref={ref}
          // dnd-kit gives its drag source the button role unless one is set.
          role="link"
          to="/main/profiles/folder/$folderId"
          params={{ folderId: folder.id }}
          color="neutral"
          icon={<FolderRounded />}
          label={folder.name}
          description={m.profile_nav_count_profiles({
            count: folder.members.length,
          })}
          data-slot="profiles-folder-nav-item"
          data-dragging={isDragging || undefined}
          data-drop-target={isDropTarget || undefined}
        />
      </RegisterContextMenuTrigger>

      <RegisterContextMenuContent>
        <ContextMenuItem onClick={() => openRenameFolder(folder.id)}>
          <EditSquareOutlineRounded className="size-4" />
          <span>{m.profile_folder_rename_title()}</span>
        </ContextMenuItem>

        <ContextMenuItem disabled={index === 0} onClick={() => moveBy(-1)}>
          <ArrowUpwardRounded className="size-4" />
          <span>{m.profile_folder_move_up()}</span>
        </ContextMenuItem>

        <ContextMenuItem
          disabled={index === count - 1}
          onClick={() => moveBy(1)}
        >
          <ArrowDownwardRounded className="size-4" />
          <span>{m.profile_folder_move_down()}</span>
        </ContextMenuItem>

        <ContextMenuItem onClick={() => deleteFolder(folder.id)}>
          <DeleteForeverOutlineRounded className="size-4" />
          <span>{m.profile_folder_delete_title()}</span>
        </ContextMenuItem>
      </RegisterContextMenuContent>
    </RegisterContextMenu>
  )
}
