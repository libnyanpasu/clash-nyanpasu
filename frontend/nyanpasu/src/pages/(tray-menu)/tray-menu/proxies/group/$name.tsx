import BoltRounded from '~icons/material-symbols/bolt-rounded'
import { useMemo } from 'react'
import { useScrollAreaViewport } from '@nyanpasu/ui/scroll-area'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import { useBlockTask } from '@/components/providers/block-task-provider'
import DelayChip from '@/components/proxies/delay-chip'
import { m } from '@/paraglide/messages'
import { useLockFn } from '@nyanpasu/hooks'
import {
  ClashProxiesQueryGroupItem,
  ClashProxiesQueryProxyItem,
  useClashProxies,
  useProxyMode,
} from '@nyanpasu/query'
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

const ProxyButton = ({
  proxy,
  selectable,
  onSelect,
}: {
  proxy: ClashProxiesQueryProxyItem
  selectable: boolean
  onSelect: (proxy: ClashProxiesQueryProxyItem) => Promise<void>
}) => {
  const currentDelay = useMemo(() => {
    if (proxy.history.length > 0) {
      return proxy.history[proxy.history.length - 1].delay
    }

    return -1
  }, [proxy.history])

  const handleClick = useLockFn(async () => {
    await onSelect(proxy)
  })

  return (
    <ActionButton
      className="w-full data-[selectable=false]:cursor-default"
      data-selectable={String(selectable)}
      // The core picks this group's member on its own.
      disabled={!selectable}
      onClick={handleClick}
    >
      <TextMarquee className="min-w-0 flex-1">{proxy.name}</TextMarquee>

      {currentDelay > 0 && <DelayChip delay={currentDelay} />}
    </ActionButton>
  )
}

function RouteComponent() {
  const { name: proxyGroupName } = Route.useParams()

  const {
    proxies: { data: proxies },
    selectProxy,
  } = useClashProxies()

  const { value: proxyMode } = useProxyMode()

  const currentGroup = useMemo<ClashProxiesQueryGroupItem | undefined>(() => {
    if (proxyMode.global) {
      return proxies?.global ?? undefined
    }

    return proxies?.groups.find((group) => group.name === proxyGroupName)
  }, [proxies, proxyGroupName, proxyMode])

  const handleSelectProxy = async (proxy: ClashProxiesQueryProxyItem) => {
    if (!currentGroup) {
      return
    }

    try {
      await selectProxy(currentGroup.name, proxy.name)
    } catch (error) {
      // A dialog would take focus and dismiss the tray menu; the frontend
      // error reporter records this in the application log.
      console.error('[tray-menu] failed to select proxy', error)
    }
  }

  const { viewportRef } = useScrollAreaViewport()

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
          const name = currentGroup?.all?.[virtualItem.index]
          const proxy = name ? proxies?.nodes[name] : undefined

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
              data-active={String(name === currentGroup?.now)}
            >
              <ProxyButton
                proxy={proxy}
                selectable={currentGroup?.capabilities.select ?? false}
                onSelect={handleSelectProxy}
              />
            </div>
          )
        })}
      </div>
    </div>
  )
}
