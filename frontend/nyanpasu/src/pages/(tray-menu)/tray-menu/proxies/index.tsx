import { useMemo } from 'react'
import DelayChip from '@/components/proxies/delay-chip'
import { CacheImage } from '@/components/ui/image'
import { ScrollArea } from '@/components/ui/scroll-area'
import TextMarquee from '@/components/ui/text-marquee'
import { m } from '@/paraglide/messages'
import { ClashProxiesQueryGroupItem, useClashProxies } from '@interface/ipc'
import { createFileRoute, Link } from '@tanstack/react-router'
import { ActionButton } from '../_modules/action-button'
import BackButton from './_modules/back-button'

export const Route = createFileRoute('/(tray-menu)/tray-menu/proxies/')({
  component: RouteComponent,
})

const ProxyButton = ({ proxy }: { proxy: ClashProxiesQueryGroupItem }) => {
  const currentDelay = useMemo(() => {
    if (proxy.history.length > 0) {
      return proxy.history[proxy.history.length - 1].delay
    }

    if (proxy.now) {
      const nodeDelay = proxy.all
        .find((item) => item.name === proxy.now)
        ?.history.at(-1)?.delay

      if (nodeDelay !== undefined) {
        return nodeDelay
      }
    }

    return -1
  }, [proxy.history, proxy.now, proxy.all])

  return (
    <ActionButton disableClose asChild>
      <Link
        to="/tray-menu/proxies/group/$name"
        params={{
          name: proxy.name,
        }}
      >
        {proxy.icon && (
          <CacheImage
            icon={proxy.icon}
            className="size-4.5"
            loadingClassName="rounded-full"
          />
        )}

        <TextMarquee className="min-w-0 flex-1">{proxy.name}</TextMarquee>

        {currentDelay > 0 && <DelayChip delay={currentDelay} />}
      </Link>
    </ActionButton>
  )
}

function RouteComponent() {
  const {
    proxies: { data: proxies },
  } = useClashProxies()

  return (
    <ScrollArea className="h-dvh w-dvw">
      <div className="flex w-dvw flex-col gap-3 p-3">
        <BackButton to="/tray-menu">
          <span>{m.tray_menu_back_to_tray_menu()}</span>
        </BackButton>

        {proxies?.groups.map((group) => (
          <ProxyButton key={group.name} proxy={group} />
        ))}
      </div>
    </ScrollArea>
  )
}
