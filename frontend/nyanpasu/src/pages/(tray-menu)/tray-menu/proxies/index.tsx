import { useMemo } from 'react'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import DelayChip from '@/components/proxies/delay-chip'
import { CacheImage } from '@/components/ui/image'
import { m } from '@/paraglide/messages'
import {
  ClashProxiesQueryGroupItem,
  ClashProxiesQueryProxyItem,
  useClashProxies,
} from '@nyanpasu/query'
import { createFileRoute, Link } from '@tanstack/react-router'
import { ActionButton } from '../_modules/action-button'
import BackButton from './_modules/back-button'

export const Route = createFileRoute('/(tray-menu)/tray-menu/proxies/')({
  component: RouteComponent,
})

const ProxyButton = ({
  proxy,
  nodes,
}: {
  proxy: ClashProxiesQueryGroupItem
  nodes: Record<string, ClashProxiesQueryProxyItem>
}) => {
  const currentDelay = useMemo(() => {
    const history = nodes[proxy.name]?.history ?? []

    if (history.length > 0) {
      return history[history.length - 1].delay
    }

    if (proxy.now) {
      const nodeDelay = nodes[proxy.now]?.history.at(-1)?.delay

      if (nodeDelay !== undefined) {
        return nodeDelay
      }
    }

    return -1
  }, [proxy.name, proxy.now, nodes])

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
          <ProxyButton key={group.name} proxy={group} nodes={proxies.nodes} />
        ))}
      </div>
    </ScrollArea>
  )
}
