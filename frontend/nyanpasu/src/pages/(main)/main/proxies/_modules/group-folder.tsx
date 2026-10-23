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
import type { ProxyGroupFolder } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { useGroupLayout } from './use-group-layout'

/**
 * A folder in the sidebar: one rounded block with a heading that opens and
 * closes it, and the folder's groups after it. Only the ends of the block get
 * the large radius.
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

  return (
    <div
      role="group"
      aria-label={folder.name}
      className={cn(
        'flex flex-col gap-0.5',
        '[&>*:first-child:not(:only-child)]:rounded-b-sm',
        '[&>*:last-child:not(:only-child)]:rounded-t-sm',
        '[&>*:not(:first-child):not(:last-child)]:rounded-sm',
      )}
      data-slot="proxies-folder"
    >
      <RegisterContextMenu>
        <RegisterContextMenuTrigger asChild>
          <NavigationListItem
            active={false}
            aria-expanded={expanded}
            onClick={onToggle}
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

      {expanded &&
        (folder.members.length > 0 ? (
          children
        ) : (
          <p
            className="bg-surface text-on-surface-variant px-4 py-3 text-xs"
            data-slot="proxies-folder-empty"
          >
            {m.proxies_nav_folder_empty()}
          </p>
        ))}
    </div>
  )
}
