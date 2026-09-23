import TextMarquee from '@/components/ui/text-marquee'
import { m } from '@/paraglide/messages'
import type {
  Proxies_Serialize,
  ProxyGroupItem_Serialize,
} from '@nyanpasu/interface'
import DelayChip from './delay-chip'
import { getGroupSelectedDelay } from './group-delay'

export default function GroupSummary({
  group,
  proxies,
}: {
  group: ProxyGroupItem_Serialize
  proxies: Proxies_Serialize
}) {
  const delay = getGroupSelectedDelay(group, proxies)

  return (
    <div className="text-on-surface-variant flex min-w-0 items-center gap-1.5 text-xs">
      <span className="bg-secondary-container text-on-secondary-container shrink-0 rounded-full px-1.5 py-0.5 text-[10px]">
        {group.type}
      </span>
      {group.now && (
        <>
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
