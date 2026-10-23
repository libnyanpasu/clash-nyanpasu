import { PageTitle } from '@/components/page-title'
import type { ProfileFolder } from '@nyanpasu/rpc/types'
import { useNavigate } from '@tanstack/react-router'
import FolderActionsMenu from '../../../_modules/folder-actions-menu'

/** The page title of a folder, with its rename and delete menu. */
export default function FolderHeader({ folder }: { folder: ProfileFolder }) {
  const navigate = useNavigate()

  // The folder is gone, and with it this page.
  const handleDeleted = () =>
    navigate({ to: '/main/profiles/$type', params: { type: 'profile' } })

  return (
    <PageTitle
      backTo="/main/profiles"
      actions={
        <FolderActionsMenu folder={folder.id} onDeleted={handleDeleted} />
      }
    >
      {folder.name}
    </PageTitle>
  )
}
