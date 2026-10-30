import EditDocumentOutlineRounded from '~icons/material-symbols/edit-document-outline-rounded'
import FolderOpenOutlineRounded from '~icons/material-symbols/folder-open-outline-rounded'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { useLockFn } from '@nyanpasu/hooks'
import { type ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { hasMaterializedFile } from '../../../_modules/utils'
import { DetailRow, DetailSection } from './detail-section'
import { useShowInFileManager } from './use-show-in-file-manager'

/** The file the profile owns on disk. */
export default function FileSection({
  profile,
}: {
  profile: ProfileItem_Serialize
}) {
  const editContent = useLockFn(async () => {
    await rpc.createEditorWindow('profile', profile.uid)
  })

  const showInFileManager = useShowInFileManager(profile)

  if (!hasMaterializedFile(profile)) {
    return null
  }

  return (
    <DetailSection title={m.profile_detail_section_file()}>
      <DetailRow
        label={m.profile_detail_edit_content()}
        trailing={
          <EditDocumentOutlineRounded
            className="text-on-surface-variant size-5 shrink-0"
            aria-hidden
          />
        }
        onClick={editContent}
      />

      <DetailRow
        label={m.profile_detail_show_in_file_manager()}
        trailing={
          <FolderOpenOutlineRounded
            className="text-on-surface-variant size-5 shrink-0"
            aria-hidden
          />
        }
        onClick={showInFileManager}
      />
    </DetailSection>
  )
}
