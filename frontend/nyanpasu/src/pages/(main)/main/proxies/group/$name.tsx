import ArrowDownwardAltRounded from '~icons/material-symbols/arrow-downward-alt-rounded'
import ArrowUpwardAltRounded from '~icons/material-symbols/arrow-upward-alt-rounded'
import Radar from '~icons/material-symbols/radar'
import { filesize } from 'filesize'
import { useCallback, useMemo } from 'react'
import { Button } from '@/components/ui/button'
import { useScrollAreaViewport } from '@/components/ui/scroll-area'
import {
  ClashProxiesQueryGroupItem,
  ClashProxiesQueryProxyItem,
  useClashProxies,
  useProxyMode,
} from '@nyanpasu/interface'
import { useContainerBreakpointValue } from '@nyanpasu/utils'
import { createFileRoute } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useGroupTrafficSpeed } from '../_modules/hooks'
import DelayTestButton from './_modules/delay-test-button'
import GroupHeader from './_modules/group-header'
import ProxyNodeButton from './_modules/proxy-node-button'

export const Route = createFileRoute('/(main)/main/proxies/group/$name')({
  component: RouteComponent,
})

// Subscribes to connection samples on its own so each sample re-renders only
// this label, not the node grid.
function GroupTrafficSpeed({ groupName }: { groupName?: string }) {
  const speed = useGroupTrafficSpeed(groupName)

  return (
    <>
      <div className="flex items-center">
        <ArrowDownwardAltRounded className="size-6" />

        <span className="text-sm">
          {filesize(speed.download, {
            standard: 'iec',
          })}
          /s
        </span>
      </div>

      <div className="flex items-center">
        <ArrowUpwardAltRounded className="size-6" />

        <span className="text-sm">
          {filesize(speed.upload, {
            standard: 'iec',
          })}
          /s
        </span>
      </div>
    </>
  )
}

function RouteComponent() {
  const { name: proxyGroupName } = Route.useParams()

  const {
    proxies: { data: proxies },
    selectProxy,
    updateProxiesDelay: { mutateAsync: mutateProxyDelay },
  } = useClashProxies()

  const { value: proxyMode } = useProxyMode()

  const currentGroup = useMemo<ClashProxiesQueryGroupItem | undefined>(() => {
    if (proxyMode.global) {
      return proxies?.global
    }

    return proxies?.groups.find((group) => group.name === proxyGroupName)
  }, [proxies, proxyGroupName, proxyMode])

  const groupName = currentGroup?.name

  const handleSelectProxy = useCallback(
    async (proxy: ClashProxiesQueryProxyItem) => {
      if (groupName) {
        await selectProxy(groupName, proxy.name)
      }
    },
    [groupName, selectProxy],
  )

  const handleDelayTest = useCallback(
    async (proxy: ClashProxiesQueryProxyItem) => {
      await mutateProxyDelay([proxy.name, proxy.provider])
    },
    [mutateProxyDelay],
  )

  const { viewportRef } = useScrollAreaViewport()

  // define the number of lanes based on the container breakpoint
  const lanes = useContainerBreakpointValue(
    viewportRef,
    {
      xs: 2,
      sm: 3,
      md: 4,
      lg: 5,
      xl: 6,
    },
    4,
  )

  const virtualizer = useVirtualizer({
    count: currentGroup?.all?.length || 0,
    getScrollElement: () => viewportRef.current,
    estimateSize: () => 60,
    overscan: 5,
    lanes,
    measureElement: (element) => element?.getBoundingClientRect().height,
  })

  const virtualItems = virtualizer.getVirtualItems()

  const handleScrollToCurrentNode = useCallback(() => {
    const index = currentGroup?.all?.findIndex(
      (proxy) => proxy.name === currentGroup?.now,
    )

    // unwarp undefined index
    if (index !== undefined) {
      virtualizer.scrollToIndex(index, {
        align: 'center',
        behavior: 'smooth',
      })
    }
  }, [currentGroup?.all, currentGroup?.now, virtualizer])

  return (
    <>
      <GroupHeader>
        <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1">
          <div className="flex max-w-full min-w-0 flex-col gap-1">
            <div className="truncate" title={currentGroup?.name}>
              {currentGroup?.name}
            </div>
          </div>

          <GroupTrafficSpeed groupName={currentGroup?.name} />
        </div>

        <div className="flex-1" />

        <Button
          icon
          className="size-8 shrink-0"
          onClick={handleScrollToCurrentNode}
        >
          <Radar className="size-4" />
        </Button>
      </GroupHeader>

      <div
        className="relative m-2"
        data-slot="proxies-virtual-list"
        style={{
          width: 'calc(100% - 16px)',
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
              className="group absolute top-0 left-0 p-1"
              style={{
                transform: `translateY(${virtualItem.start}px)`,
                width: `${100 / lanes}%`,
                left: `${virtualItem.lane * (100 / lanes)}%`,
              }}
              data-index={virtualItem.index}
              data-slot="proxies-virtual-item"
              data-active={String(proxy.name === currentGroup?.now)}
            >
              <ProxyNodeButton
                proxy={proxy}
                onSelect={handleSelectProxy}
                onDelayTest={handleDelayTest}
              />
            </div>
          )
        })}
      </div>

      <DelayTestButton />
    </>
  )
}
