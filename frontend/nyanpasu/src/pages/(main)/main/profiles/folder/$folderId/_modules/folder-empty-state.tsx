import FolderOpenOutlineRounded from '~icons/material-symbols/folder-open-outline-rounded'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import { m } from '@/paraglide/messages'

/** A folder without profiles. */
export default function FolderEmptyState() {
  return (
    <EmptyState
      className="flex-1"
      icon={FolderOpenOutlineRounded}
      message={m.profile_folder_empty_title()}
      description={m.profile_folder_empty_description()}
      data-slot="profiles-folder-empty-state"
    />
  )
}
