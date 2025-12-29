import { AppContentScrollArea } from '@/components/ui/scroll-area'
import useIsMobile from '@/hooks/use-is-moblie'
import { createFileRoute } from '@tanstack/react-router'
import ProxiesNavigate from './_modules/proxies-navigate'

export const Route = createFileRoute('/(experimental)/experimental/proxies/')({
  component: RouteComponent,
})

function RouteComponent() {
  const isMobile = useIsMobile()

  if (!isMobile) {
    return null
  }

  return (
    <AppContentScrollArea
      className="bg-surface-variant/10"
      data-slot="proxies-sidebar-scroll-area"
    >
      <ProxiesNavigate />
    </AppContentScrollArea>
  )
}
