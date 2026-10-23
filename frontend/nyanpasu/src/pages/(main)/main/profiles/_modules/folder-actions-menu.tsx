import DeleteForeverOutlineRounded from '~icons/material-symbols/delete-forever-outline-rounded'
import EditSquareOutlineRounded from '~icons/material-symbols/edit-square-outline-rounded'
import MoreVertRounded from '~icons/material-symbols/more-vert-rounded'
import { Button } from '@nyanpasu/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import type { FolderId } from '@nyanpasu/rpc/types'
import { useProfileLayout } from './use-profile-layout'

/** The overflow menu of a folder: rename it, or delete it. */
export default function FolderActionsMenu({
  folder,
  onDeleted,
}: {
  folder: FolderId
  /** Called once the folder is deleted, not when the user declines. */
  onDeleted?: () => void
}) {
  const { openRenameFolder, deleteFolder } = useProfileLayout()

  const handleRename = () => openRenameFolder(folder)

  const handleDelete = async () => {
    if (await deleteFolder(folder)) {
      onDeleted?.()
    }
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          icon
          className="size-9 shrink-0"
          aria-label={m.profile_detail_more_actions()}
          data-slot="profiles-folder-actions"
        >
          <MoreVertRounded className="size-5" aria-hidden />
        </Button>
      </DropdownMenuTrigger>

      <DropdownMenuContent align="end">
        <DropdownMenuItem onSelect={handleRename}>
          <EditSquareOutlineRounded className="size-4" aria-hidden />
          <span>{m.profile_folder_rename_title()}</span>
        </DropdownMenuItem>

        <DropdownMenuItem onSelect={handleDelete}>
          <DeleteForeverOutlineRounded className="size-4" aria-hidden />
          <span>{m.profile_folder_delete_title()}</span>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
