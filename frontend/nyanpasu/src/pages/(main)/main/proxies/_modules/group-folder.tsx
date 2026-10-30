import ArrowDownwardRounded from '~icons/material-symbols/arrow-downward-rounded'
import ArrowUpwardRounded from '~icons/material-symbols/arrow-upward-rounded'
import DeleteForeverOutlineRounded from '~icons/material-symbols/delete-forever-outline-rounded'
import EditSquareOutlineRounded from '~icons/material-symbols/edit-square-outline-rounded'
import ExpandMoreRounded from '~icons/material-symbols/expand-more-rounded'
import FolderRounded from '~icons/material-symbols/folder-rounded'
import type { ReactNode } from 'react'
import { ContextMenuItem } from '@nyanpasu/ui/context-menu'
import {
  NavigationListItem,
  NavigationListItemIcon,
  NavigationListItemText,
} from '@nyanpasu/ui/navigation-list'
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider'
import { m } from '@/paraglide/messages'
import { FOLDER_ITEM_DRAG_SENSORS } from '@/utils/dnd-sensors'
import { pointerIntersection } from '@dnd-kit/collision'
import { useDroppable } from '@dnd-kit/react'
import { useSortable } from '@dnd-kit/react/sortable'
import type { ProxyGroupFolder } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import {
  FOLDER_DRAG_TYPE,
  folderDragId,
  folderHeadingId,
  FOLDERS_DRAG_GROUP,
  GROUP_DRAG_TYPE,
  HEADING_DRAG_TYPE,
  SECTION_DRAG_TYPE,
  sectionId,
} from './resolve-group-drop'
import { useGroupLayout } from './use-group-layout'

/**
 * dnd-kit's `CollisionPriority.Lowest`, which `@dnd-kit/abstract` exports but
 * this app does not depend on directly. A group under the pointer wins over
 * the list around it.
 */
const SECTION_COLLISION_PRIORITY = 0

/**
 * A folder in the sidebar: one rounded block with a heading that opens and
 * closes it, and the folder's groups after it. Only the ends of the block get
 * the large radius. The heading drags the block to reorder the folders, and
 * takes a group dropped on it.
 */
export default function GroupFolder({
  folder,
  index,
  count,
  expanded,
  onToggle,
  children,
}: {
  folder: ProxyGroupFolder
  /** Where the folder sits among all folders. */
  index: number
  /** How many folders there are. */
  count: number
  expanded: boolean
  onToggle: () => void
  /** The folder's group items. */
  children: ReactNode
}) {
  const { openRenameFolder, deleteFolder, moveFolderBy } = useGroupLayout()

  const {
    ref: blockRef,
    handleRef,
    isDragging,
  } = useSortable({
    id: folderDragId(folder.id),
    index,
    group: FOLDERS_DRAG_GROUP,
    type: FOLDER_DRAG_TYPE,
    accept: [FOLDER_DRAG_TYPE],
    sensors: FOLDER_ITEM_DRAG_SENSORS,
  })

  // The heading takes a group as a drop at the end, open or closed.
  const { ref: headingDropRef, isDropTarget } = useDroppable({
    id: folderHeadingId(folder.id),
    type: HEADING_DRAG_TYPE,
    accept: GROUP_DRAG_TYPE,
    collisionDetector: pointerIntersection,
  })

  // An open folder's list takes a group into the place under the pointer, and
  // into an empty folder, which has no group to drop on.
  const { ref: listRef } = useDroppable({
    id: sectionId(folder.id),
    type: SECTION_DRAG_TYPE,
    accept: GROUP_DRAG_TYPE,
    collisionPriority: SECTION_COLLISION_PRIORITY,
    collisionDetector: pointerIntersection,
    disabled: !expanded,
  })

  return (
    <div
      ref={blockRef}
      role="group"
      aria-label={folder.name}
      className="flex flex-col gap-0.5"
      data-slot="proxies-folder"
      data-dragging={isDragging || undefined}
    >
      <RegisterContextMenu>
        <RegisterContextMenuTrigger asChild>
          <NavigationListItem
            ref={(element: HTMLButtonElement | null) => {
              headingDropRef(element)
              handleRef(element)
            }}
            active={false}
            aria-expanded={expanded}
            // The block's top corners stay large; below the heading is a list.
            className={cn(expanded && 'rounded-b-sm')}
            onClick={onToggle}
            data-drop-target={isDropTarget || undefined}
          >
            <ExpandMoreRounded
              className={cn(
                'text-on-surface-variant size-5 shrink-0 transition-transform duration-200',
                !expanded && '-rotate-90',
              )}
              aria-hidden
            />

            <NavigationListItemIcon color="neutral">
              <FolderRounded />
            </NavigationListItemIcon>

            <NavigationListItemText
              headline={folder.name}
              supporting={m.proxies_nav_group_count({
                count: folder.members.length,
              })}
            />
          </NavigationListItem>
        </RegisterContextMenuTrigger>

        <RegisterContextMenuContent>
          <ContextMenuItem onClick={() => openRenameFolder(folder.id)}>
            <EditSquareOutlineRounded className="size-4" />
            <span>{m.profile_folder_rename_title()}</span>
          </ContextMenuItem>

          <ContextMenuItem
            disabled={index === 0}
            onClick={() => moveFolderBy(folder.id, -1)}
          >
            <ArrowUpwardRounded className="size-4" />
            <span>{m.profile_folder_move_up()}</span>
          </ContextMenuItem>

          <ContextMenuItem
            disabled={index === count - 1}
            onClick={() => moveFolderBy(folder.id, 1)}
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

      {expanded && (
        <div
          ref={listRef}
          className={cn(
            'flex flex-col gap-0.5',
            '[&>*]:rounded-sm [&>*:last-child]:rounded-b-3xl',
          )}
          data-slot="proxies-folder-list"
        >
          {folder.members.length > 0 ? (
            children
          ) : (
            <p
              className="bg-surface text-on-surface-variant px-4 py-3 text-xs"
              data-slot="proxies-folder-empty"
            >
              {m.proxies_nav_folder_empty()}
            </p>
          )}
        </div>
      )}
    </div>
  )
}
