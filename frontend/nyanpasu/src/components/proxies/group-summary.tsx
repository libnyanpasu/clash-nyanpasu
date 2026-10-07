import KeepRounded from '~icons/material-symbols/keep-rounded'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import { m } from '@/paraglide/messages'
import { memberDelay, useSetting } from '@nyanpasu/query'
import type { Proxies_Serialize, ProxyGroup } from '@nyanpasu/rpc/types'
import DelayChip from './delay-chip'

export default function GroupSummary({
  group,
  proxies,
}: {
  group: ProxyGroup
  proxies: Proxies_Serialize
}) {
  const { value: defaultUrl } = useSetting('default_latency_test')

  const delay = group.now
    ? memberDelay(group.now, group, proxies, defaultUrl ?? '')
    : undefined

  return (
    <div className="text-on-surface-variant flex min-w-0 items-center gap-1.5 text-xs">
      <span className="bg-secondary-container text-on-secondary-container shrink-0 rounded-full px-1.5 py-0.5 text-[10px]">
        {group.type}
      </span>
      {group.now && (
        <>
          {group.fixed && (
            <span
              className="shrink-0"
              title={group.fixed}
              data-slot="group-summary-fixed-icon"
            >
              <KeepRounded className="size-3.5" />
            </span>
          )}

          <div className="min-w-0 flex-1" title={group.now}>
            <TextMarquee className="w-full">{group.now}</TextMarquee>
          </div>
          <span className="ml-auto shrink-0 tabular-nums">
            {delay === undefined ? (
              <span title={m.proxies_delay_history_empty()}>—</span>
            ) : delay <= 0 ? (
              <span
                className="text-red-500"
                title={m.proxies_delay_history_failed()}
              >
                ∞
              </span>
            ) : (
              <DelayChip delay={delay} />
            )}
          </span>
        </>
      )}
    </div>
  )
}
