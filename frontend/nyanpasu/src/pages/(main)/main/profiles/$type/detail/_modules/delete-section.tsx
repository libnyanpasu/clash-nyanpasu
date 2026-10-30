import { m } from '@/paraglide/messages'
import { useCurrentProfileUid } from '@nyanpasu/query'
import { type ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { useDeleteProfile } from '../../../_modules/use-delete-profile'
import { SettingsGroup } from '../../../../_modules/settings-card'
import { DetailRow } from './detail-section'

/** The one destructive action, kept apart from the rest. */
export default function DeleteSection({
  profile,
  onDeleted,
}: {
  profile: ProfileItem_Serialize
  onDeleted: () => void | Promise<void>
}) {
  const isActive = useCurrentProfileUid() === profile.uid

  const deleteProfile = useDeleteProfile(profile, {
    description: (isActive
      ? m.profile_detail_delete_confirm_active
      : m.profile_detail_delete_confirm)({ name: profile.name }),
    onSuccess: onDeleted,
  })

  return (
    <SettingsGroup data-slot="profile-detail-delete">
      <DetailRow
        destructive
        label={m.profile_delete_title()}
        description={m.profile_detail_delete_description()}
        loading={deleteProfile.isPending}
        trailing={null}
        onClick={deleteProfile.handleClick}
      />
    </SettingsGroup>
  )
}
