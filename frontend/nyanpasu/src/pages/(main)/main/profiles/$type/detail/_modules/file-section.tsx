import EditDocumentOutlineRounded from '~icons/material-symbols/edit-document-outline-rounded'
import FolderOpenOutlineRounded from '~icons/material-symbols/folder-open-outline-rounded'
import OpenInNewRounded from '~icons/material-symbols/open-in-new-rounded'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useMatchesPlatform } from '@nyanpasu/platform/react'
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

  const isDesktop = useMatchesPlatform('desktop')
  const showInFileManager = useShowInFileManager(profile)
  const openFile = useLockFn(async () => {
    try {
      await rpc.openProfile(profile.uid)
    } catch (error) {
      await message(formatError(error), {
        title: m.profile_detail_open_file(),
        kind: 'error',
        error,
      })
    }
  })

  if (!isDesktop || !hasMaterializedFile(profile)) {
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
        label={m.profile_detail_open_file()}
        trailing={
          <OpenInNewRounded
            className="text-on-surface-variant size-5 shrink-0"
            aria-hidden
          />
        }
        onClick={openFile}
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
