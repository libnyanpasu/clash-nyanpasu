import { m } from '@/paraglide/messages'
import { type ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { DetailSwitchRow } from './detail-section'
import {
  UNCHANGED_REMOTE_OPTIONS,
  useRemoteOptionsPatch,
} from './use-remote-options-patch'

// Mirrors the backend default; used when auto update is switched on, as the
// profile keeps no interval while it is off.
// TODO: prefer the subscription's suggested interval (`profile-update-interval`)
// once the profile stores it; today it is applied only at import.
const DEFAULT_UPDATE_INTERVAL_MINUTES = 120

// The backend turns auto update off with an interval of 0 minutes.
const AUTO_UPDATE_OFF_MINUTES = 0

/** Turns the automatic updates of a remote profile on or off. */
export function AutoUpdateSwitch({
  profile,
  intervalMinutes,
}: {
  profile: ProfileItem_Serialize
  intervalMinutes: number
}) {
  const { patch, isPending } = useRemoteOptionsPatch(profile.uid, 'auto-update')

  return (
    <DetailSwitchRow
      label={m.profile_update_option_auto_update_label()}
      checked={intervalMinutes > AUTO_UPDATE_OFF_MINUTES}
      loading={isPending}
      onCheckedChange={(checked) =>
        patch({
          ...UNCHANGED_REMOTE_OPTIONS,
          update_interval_minutes: checked
            ? DEFAULT_UPDATE_INTERVAL_MINUTES
            : AUTO_UPDATE_OFF_MINUTES,
        })
      }
    />
  )
}

/** Whether a remote profile is downloaded through the system or Clash proxy. */
export function ProxySwitch({
  profile,
  channel,
  checked,
}: {
  profile: ProfileItem_Serialize
  channel: 'with_proxy' | 'self_proxy'
  checked: boolean
}) {
  const { patch, isPending } = useRemoteOptionsPatch(profile.uid, channel)

  return (
    <DetailSwitchRow
      label={
        channel === 'with_proxy'
          ? m.profile_with_proxy_label()
          : m.profile_self_proxy_label()
      }
      checked={checked}
      loading={isPending}
      onCheckedChange={(next) =>
        patch({ ...UNCHANGED_REMOTE_OPTIONS, [channel]: next })
      }
    />
  )
}
