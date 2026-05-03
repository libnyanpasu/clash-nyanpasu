import BoltRounded from '~icons/material-symbols/bolt-rounded'
import { useMemo } from 'react'
import {
  ClashProxiesQueryGroupItem,
  ClashProxiesQueryProxyItem,
  useClashProxies,
  useProxyMode,
} from '@nyanpasu/interface/ipc'
import { useBlockTask } from '@/components/providers/block-task-provider'
import DelayChip from '@/components/proxies/delay-chip'
import { useScrollArea } from '@/components/ui/scroll-area'
import TextMarquee from '@/components/ui/text-marquee'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import { createFileRoute } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'
import BackButton from '../_modules/back-button'
import { ActionButton } from '../../_modules/action-button'

export const Route = createFileRoute(
  '/(tray-menu)/tray-menu/proxies/group/$name',
)({
  component: RouteComponent,
})

const DelayTestButton = () => {
  const { name } = Route.useParams()

  const { updateGroupDelay } = useClashProxies()

  const blockTask = useBlockTask(`tray-delay-group-test-${name}`, async () => {
    await updateGroupDelay.mutateAsync([name])
  })

  return (
    <ActionButton
      className="w-10 shrink-0 justify-center backdrop-blur-lg"
      disableClose
      onClick={() => blockTask.execute()}
      loading={blockTask.isPending}
    >
      <BoltRounded />
    </ActionButton>
  )
}

const ProxyButton = ({ proxy }: { proxy: ClashProxiesQueryProxyItem }) => {
  const currentDelay = useMemo(() => {
    if (proxy.history.length > 0) {
      return proxy.history[proxy.history.length - 1].delay
    }

    return -1
  }, [proxy.history])

  const handleClick = useLockFn(async () => {
    await proxy.mutateSelect()
  })

  return (
    <ActionButton className="w-full" onClick={handleClick}>
      <TextMarquee className="min-w-0 flex-1">{proxy.name}</TextMarquee>

      {currentDelay > 0 && <DelayChip delay={currentDelay} />}
    </ActionButton>
  )
}

function RouteComponent() {
  const { name: proxyGroupName } = Route.useParams()

  const {
    proxies: { data: proxies },
  } = useClashProxies()

  const { value: proxyMode } = useProxyMode()

  const currentGroup = useMemo<ClashProxiesQueryGroupItem | undefined>(() => {
    if (proxyMode.global) {
      return proxies?.global
    }

    return proxies?.groups.find((group) => group.name === proxyGroupName)
  }, [proxies, proxyGroupName, proxyMode])

  const { viewportRef } = useScrollArea()

  const virtualizer = useVirtualizer({
    count: currentGroup?.all?.length || 0,
    getScrollElement: () => viewportRef.current,
    estimateSize: () => 60,
    overscan: 5,
    measureElement: (element) => element?.getBoundingClientRect().height,
  })

  const virtualItems = virtualizer.getVirtualItems()

  return (
    <div className="w-dvw p-3">
      <div className="sticky top-3 z-10 flex gap-2">
        <BackButton className="block" to="/tray-menu/proxies">
          <span>{m.tray_menu_back_to_proxies_menu()}</span>
        </BackButton>

        <DelayTestButton />
      </div>

      <div
        className="relative"
        data-slot="proxies-virtual-list"
        style={{
          height: `${virtualizer.getTotalSize()}px`,
        }}
      >
        {virtualItems.map((virtualItem) => {
          const proxy = currentGroup?.all?.[virtualItem.index]

          if (!proxy) {
            return null
          }

          return (
            <div
              key={virtualItem.index}
              ref={virtualizer.measureElement}
              className={cn(
                'absolute top-0 left-0 w-full',
                'flex flex-col pt-3',
              )}
              style={{
                transform: `translateY(${virtualItem.start}px)`,
              }}
              data-index={virtualItem.index}
              data-slot="proxies-virtual-item"
              data-active={String(proxy.name === currentGroup?.now)}
            >
              <ProxyButton proxy={proxy} />
            </div>
          )
        })}
      </div>
    </div>
  )
}
