import AddRounded from '~icons/material-symbols/add-rounded'
import CheckRounded from '~icons/material-symbols/check-rounded'
import DriveFileMoveOutlineRounded from '~icons/material-symbols/drive-file-move-outline-rounded'
import FolderOffOutlineRounded from '~icons/material-symbols/folder-off-outline-rounded'
import FolderRounded from '~icons/material-symbols/folder-rounded'
import { Fragment, type ComponentType, type SVGProps } from 'react'
import {
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
} from '@nyanpasu/ui/context-menu'
import {
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import type { FolderId, ProfileFolder, ProfileId } from '@nyanpasu/rpc/types'
import { useProfileLayout } from './use-profile-layout'

/**
 * What a "Move to folder" menu needs. It is read where the profile is shown,
 * because a context menu renders outside the layout provider.
 */
export type ProfileMove = {
  folders: ProfileFolder[]
  /** The folder the profile is in. */
  folder: ProfileFolder | null
  /** Moves the profile into `destination`, or out of its folder for `null`. */
  handleMove: (destination: FolderId | null) => void
  /** Asks for a name, then moves the profile into the new folder. */
  handleCreate: () => void
}

export const useProfileMove = (uid: ProfileId): ProfileMove => {
  const { layout, moveProfile, openCreateFolder } = useProfileLayout()

  const folders = layout?.folders ?? []

  return {
    folders,
    folder: folders.find((folder) => folder.members.includes(uid)) ?? null,
    handleMove: (destination) => {
      moveProfile(uid, destination)
    },
    handleCreate: () => openCreateFolder({ members: [uid] }),
  }
}

type MoveEntry = {
  key: string
  label: string
  icon: ComponentType<SVGProps<SVGSVGElement>>
  disabled?: boolean
  /** Starts the entries that act on folders rather than pick one. */
  separated?: boolean
  select: () => void
}

const entriesOf = ({
  folders,
  folder,
  handleMove,
  handleCreate,
}: ProfileMove): MoveEntry[] => [
  ...folders.map((item) => ({
    key: item.id,
    label: item.name,
    icon: item.id === folder?.id ? CheckRounded : FolderRounded,
    disabled: item.id === folder?.id,
    select: () => handleMove(item.id),
  })),
  {
    key: 'new',
    label: m.profile_move_new_folder(),
    icon: AddRounded,
    separated: folders.length > 0,
    select: handleCreate,
  },
  ...(folder
    ? [
        {
          key: 'remove',
          label: m.profile_move_remove_from_folder(),
          icon: FolderOffOutlineRounded,
          select: () => handleMove(null),
        },
      ]
    : []),
]

/** "Move to folder" as a submenu of a context menu. */
export function ProfileMoveContextMenu({
  move,
  disabled,
}: {
  move: ProfileMove
  disabled?: boolean
}) {
  return (
    <ContextMenuSub>
      <ContextMenuSubTrigger disabled={disabled}>
        <span className="flex items-center gap-2">
          <DriveFileMoveOutlineRounded className="size-4" />
          {m.profile_move_to_folder()}
        </span>
      </ContextMenuSubTrigger>

      <ContextMenuSubContent>
        {entriesOf(move).map(
          ({ key, label, icon: Icon, disabled, separated, select }) => (
            <Fragment key={key}>
              {separated && <ContextMenuSeparator />}

              <ContextMenuItem disabled={disabled} onSelect={select}>
                <Icon className="size-4" />
                <span className="truncate">{label}</span>
              </ContextMenuItem>
            </Fragment>
          ),
        )}
      </ContextMenuSubContent>
    </ContextMenuSub>
  )
}

/** "Move to folder" as a submenu of a dropdown menu. */
export function ProfileMoveDropdownMenu({ move }: { move: ProfileMove }) {
  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger>
        <span className="flex items-center gap-2">
          <DriveFileMoveOutlineRounded className="size-4" aria-hidden />
          {m.profile_move_to_folder()}
        </span>
      </DropdownMenuSubTrigger>

      <DropdownMenuSubContent>
        {entriesOf(move).map(
          ({ key, label, icon: Icon, disabled, separated, select }) => (
            <Fragment key={key}>
              {separated && <DropdownMenuSeparator />}

              <DropdownMenuItem disabled={disabled} onSelect={select}>
                <Icon className="size-4" aria-hidden />
                <span className="truncate">{label}</span>
              </DropdownMenuItem>
            </Fragment>
          ),
        )}
      </DropdownMenuSubContent>
    </DropdownMenuSub>
  )
}
