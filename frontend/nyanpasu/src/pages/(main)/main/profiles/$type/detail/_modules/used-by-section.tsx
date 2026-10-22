import { m } from '@/paraglide/messages'
import { isConfigItem, scopedTransformsOf } from '@nyanpasu/query'
import { type ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { Link } from '@tanstack/react-router'
import { ProfileType } from '../../../_modules/consts'
import {
  SettingsCard,
  SettingsCardContent,
} from '../../../../_modules/settings-card'
import { DetailRow, DetailSection } from './detail-section'

/**
 * Where a transform runs: the profiles whose own chain contains it, and the
 * global chain. Editing its content changes every one of them.
 */
export default function UsedBySection({
  uid,
  profiles,
  globalTransforms,
}: {
  uid: string
  profiles: ProfileItem_Serialize[]
  globalTransforms: string[]
}) {
  const hosts = profiles.filter(
    (item) => isConfigItem(item) && scopedTransformsOf(item).includes(uid),
  )

  const isGlobal = globalTransforms.includes(uid)

  return (
    <DetailSection
      data-slot="profile-detail-used-by"
      title={m.profile_detail_used_by_title()}
      description={m.profile_detail_used_by_description()}
    >
      {hosts.map((host) => (
        <DetailRow key={host.uid} label={host.name} asChild>
          <Link
            to="/main/profiles/$type/detail/$uid"
            params={{ type: ProfileType.Profile, uid: host.uid }}
          />
        </DetailRow>
      ))}

      {isGlobal && (
        <DetailRow label={m.profile_global_transform_title()} asChild>
          <Link to="/main/profiles/global" />
        </DetailRow>
      )}

      {hosts.length === 0 && !isGlobal && (
        <SettingsCard data-slot="profile-detail-used-by-empty">
          <SettingsCardContent className="text-on-surface-variant py-4 text-sm">
            {m.profile_detail_used_by_empty()}
          </SettingsCardContent>
        </SettingsCard>
      )}
    </DetailSection>
  )
}
