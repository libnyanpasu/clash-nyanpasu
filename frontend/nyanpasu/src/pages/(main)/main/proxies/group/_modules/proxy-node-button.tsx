import FlashOnRounded from '~icons/material-symbols/flash-on-rounded'
import KeepRounded from '~icons/material-symbols/keep-rounded'
import { ComponentProps, memo, MouseEvent, useMemo } from 'react'
import { Button } from '@nyanpasu/ui/button'
import HighlightText from '@nyanpasu/ui/highlight-text'
import { useBlockTask } from '@/components/providers/block-task-provider'
import DelayChip from '@/components/proxies/delay-chip'
import DelayHistory from '@/components/proxies/delay-history'
import { m } from '@/paraglide/messages'
import { useLockFn } from '@nyanpasu/hooks'
import { ClashProxiesQueryProxyItem, nodeDelayHistory } from '@nyanpasu/query'
import { cn } from '@nyanpasu/utils'

function FeatureChip({
  label,
  variant = 'feature',
}: {
  label: string
  variant?: 'type' | 'feature'
}) {
  return (
    <span
      className={cn(
        'shrink-0 rounded-full px-1.5 py-0.5 text-[9px] leading-none font-medium uppercase',
        variant === 'type'
          ? 'bg-primary/10 text-primary'
          : 'bg-secondary-container text-on-secondary-container',
      )}
    >
      {label}
    </span>
  )
}

// Memoized with stable actions: query data keeps unchanged nodes' identity
// across refetches, so only nodes whose data changed re-render.
export default memo(function ProxyNodeButton({
  proxy,
  selectable,
  fixed,
  onSelect,
  onDelayTest,
  testUrl,
  searchText = '',
  ...props
}: Omit<ComponentProps<typeof Button>, 'onClick' | 'children' | 'onSelect'> & {
  proxy: ClashProxiesQueryProxyItem
  selectable: boolean
  fixed: boolean
  onSelect: (proxy: ClashProxiesQueryProxyItem) => Promise<void>
  onDelayTest: (proxy: ClashProxiesQueryProxyItem) => Promise<void>
  testUrl: string
  /** The term highlighted in the name; empty highlights nothing. */
  searchText?: string
}) {
  const handleSelectProxy = useLockFn(async () => {
    // The core picks this group's member on its own.
    if (!selectable) {
      return
    }

    await onSelect(proxy)
  })

  const delayTask = useBlockTask(
    `proxy-delay-check-${proxy.name.toLowerCase()}`,
    async () => {
      await onDelayTest(proxy)
    },
  )

  const handleDelayClick = useLockFn(
    async (e: MouseEvent<HTMLButtonElement>) => {
      e.preventDefault()
      e.stopPropagation()

      await delayTask.execute()
    },
  )

  const history = useMemo(
    () => nodeDelayHistory(proxy, testUrl),
    [proxy, testUrl],
  )

  const currentDelay = history.at(-1)?.delay ?? -1

  return (
    <DelayHistory history={history}>
      <Button
        variant="fab"
        className={cn(
          'flex w-full flex-col justify-center gap-1 px-2 text-left',
          // The fab hover brightness filter would dim the nested chips and
          // delay button too, so a state layer below the content tints only
          // the card itself. It fades its color rather than its opacity:
          // WebKit composites an opacity animation, which moves the content
          // above it into layers and makes it jitter.
          'isolate hover:filter-none!',
          'before:absolute before:inset-0 before:-z-10 before:transition-colors',
          'hover:before:bg-on-surface/5',
          'group-data-[active=true]:bg-primary-container/75',
          'dark:group-data-[active=true]:bg-surface-variant/50',
          'group-data-[active=false]:bg-on-background/3',
          'dark:group-data-[active=false]:bg-surface/30',
          'group-data-[active=false]:shadow-none',
          'group-data-[active=false]:hover:shadow-none',
          'group-data-[active=false]:hover:bg-surface-variant/30',
          'data-[selectable=false]:cursor-default',
          'data-[selectable=false]:hover:before:bg-transparent',
        )}
        data-selectable={String(selectable)}
        // Not `disabled`: the card holds the latency control, which must stay
        // clickable.
        aria-disabled={!selectable}
        onClick={handleSelectProxy}
        {...props}
      >
        <div className="flex w-full items-center justify-between gap-2 px-2">
          <div className="truncate text-sm font-medium">
            <HighlightText searchText={searchText}>{proxy.name}</HighlightText>
          </div>

          {fixed && (
            <span
              className="text-primary shrink-0"
              title={m.proxies_group_fixed_label()}
              data-slot="proxy-node-fixed-icon"
            >
              <KeepRounded className="size-4" />
            </span>
          )}
          {/* TODO: takes up too much space and needs to be redesigned */}
          {/* <DelayHistoryBar history={proxy.history ?? []} /> */}
        </div>

        <div className="flex w-full items-center justify-between gap-2 overflow-hidden px-2">
          <div className="flex items-center gap-1 overflow-hidden">
            <FeatureChip label={proxy.type} variant="type" />
            {proxy.udp && <FeatureChip label="UDP" />}
            {proxy.xudp && <FeatureChip label="XUDP" />}
            {proxy.tfo && <FeatureChip label="TFO" />}
          </div>

          <Button
            className="grid h-4 min-w-10 shrink-0 place-content-center px-2 text-center"
            variant="raised"
            onClick={handleDelayClick}
            loading={delayTask.isPending}
            asChild
          >
            {currentDelay > 0 ? (
              <DelayChip delay={currentDelay} />
            ) : (
              <span>
                <FlashOnRounded className="py-1" />
              </span>
            )}
          </Button>
        </div>
      </Button>
    </DelayHistory>
  )
})
