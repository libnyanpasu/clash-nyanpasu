import { AnimatePresence } from 'motion/react'
import { AnimatedItem } from '@nyanpasu/ui/animated-item'
import { m } from '@/paraglide/messages'
import { getRemoteSource } from '@nyanpasu/query'
import { type ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { DetailRow, DetailSection } from './detail-section'
import { AutoUpdateSwitch, ProxySwitch } from './subscription-switches'
import SubscriptionUrlEditor from './subscription-url-editor'
import UpdateIntervalEditor from './update-interval-editor'
import UserAgentEditor from './user-agent-editor'

/** The source of a remote profile and how it is refreshed. */
export default function SubscriptionSection({
  profile,
}: {
  profile: ProfileItem_Serialize
}) {
  const remote = getRemoteSource(profile)

  if (!remote) {
    return null
  }

  const { option } = remote

  const autoUpdate = option.update_interval_minutes > 0

  return (
    <DetailSection title={m.profile_detail_section_subscription()}>
      <SubscriptionUrlEditor profile={profile} asChild>
        <DetailRow
          label={m.profile_subscription_url_label()}
          value={remote.url}
        />
      </SubscriptionUrlEditor>

      <UserAgentEditor profile={profile} asChild>
        <DetailRow
          label={m.profile_user_agent_label()}
          value={option.user_agent || m.profile_detail_user_agent_default()}
        />
      </UserAgentEditor>

      <AutoUpdateSwitch
        profile={profile}
        intervalMinutes={option.update_interval_minutes}
      />

      <AnimatePresence initial={false}>
        {autoUpdate && (
          // The group rounds this animated child by position; its row
          // inherits that shape.
          <AnimatedItem
            key="update-interval"
            data-slot="profile-detail-update-interval"
            className="rounded-extra-large *:rounded-[inherit]!"
          >
            <UpdateIntervalEditor profile={profile} asChild>
              <DetailRow
                label={m.profile_update_interval_label()}
                value={m.profile_detail_auto_update_every({
                  minutes: option.update_interval_minutes,
                })}
              />
            </UpdateIntervalEditor>
          </AnimatedItem>
        )}
      </AnimatePresence>

      <ProxySwitch
        profile={profile}
        channel="with_proxy"
        checked={option.with_proxy}
      />

      <ProxySwitch
        profile={profile}
        channel="self_proxy"
        checked={option.self_proxy}
      />
    </DetailSection>
  )
}
