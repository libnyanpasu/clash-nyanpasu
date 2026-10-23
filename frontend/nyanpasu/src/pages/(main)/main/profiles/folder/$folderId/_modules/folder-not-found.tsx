import FolderOffOutlineRounded from '~icons/material-symbols/folder-off-outline-rounded'
import { Button } from '@nyanpasu/ui/button'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import { m } from '@/paraglide/messages'
import { Link } from '@tanstack/react-router'
import { ProfileType } from '../../../_modules/consts'

/** The page of a folder that does not exist, or no longer does. */
export default function FolderNotFound() {
  return (
    <EmptyState
      className="flex-1"
      icon={FolderOffOutlineRounded}
      message={m.profile_folder_not_found()}
      data-slot="profiles-folder-not-found"
    >
      <Button variant="stroked" className="px-4" asChild>
        <Link to="/main/profiles/$type" params={{ type: ProfileType.Profile }}>
          {m.profile_nav_all_profiles()}
        </Link>
      </Button>
    </EmptyState>
  )
}
