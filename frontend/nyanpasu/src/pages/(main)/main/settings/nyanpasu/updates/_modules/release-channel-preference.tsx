import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { useNyanpasuUpdate } from '@/components/providers/nyanpasu-update-provider'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useReleaseChannel } from '@nyanpasu/query'
import { type ReleaseChannel } from '@nyanpasu/rpc/types'

export default function ReleaseChannelPreference() {
  const { query, mutation } = useReleaseChannel()
  const { isBusy } = useNyanpasuUpdate()
  const releaseChannel = query.data?.current
  const isChannelLocked = query.data?.installed === 'nightly'
  const labels: Record<ReleaseChannel, string> = {
    stable: m.release_channel_stable(),
    beta: m.release_channel_beta(),
    nightly: m.release_channel_nightly(),
  }
  const notices: Partial<Record<ReleaseChannel, string>> = {
    stable: m.release_channel_stable_notice(),
    nightly: m.release_channel_nightly_notice(),
  }
  const handleChange = useLockFn(async (channel: ReleaseChannel) => {
    try {
      await mutation.mutateAsync(channel)
    } catch (error) {
      message(formatError(error), { kind: 'error', error })
    }
  })

  return (
    <DropdownMenu align="end">
      <DropdownMenuTrigger asChild>
        <button
          data-slot="about-release-channel-preference"
          aria-label={m.release_channel_label()}
          disabled={
            !releaseChannel || isChannelLocked || isBusy || mutation.isPending
          }
          type="button"
          className="bg-surface-variant/30 dark:bg-surface-variant/10 hover:bg-surface-variant/50 flex h-16 w-full cursor-pointer items-center justify-between gap-2 rounded-[20px] p-4 text-left transition-colors disabled:cursor-not-allowed disabled:opacity-60"
        >
          <div className="flex min-w-0 flex-col gap-0.5">
            <span>{m.settings_about_channel_title()}</span>

            <span className="text-on-surface-variant truncate text-xs">
              {isChannelLocked
                ? notices.nightly
                : releaseChannel
                  ? labels[releaseChannel]
                  : null}
            </span>
          </div>

          <ArrowForwardIosRounded className="shrink-0" />
        </button>
      </DropdownMenuTrigger>

      <DropdownMenuContent>
        {(Object.keys(labels) as ReleaseChannel[]).map((channel) => (
          <DropdownMenuCheckboxItem
            key={channel}
            checked={releaseChannel === channel}
            onSelect={() => handleChange(channel)}
          >
            <span className="flex flex-col gap-0.5">
              <span>{labels[channel]}</span>
              {notices[channel] && (
                <span className="text-on-surface-variant text-xs">
                  {notices[channel]}
                </span>
              )}
            </span>
          </DropdownMenuCheckboxItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
