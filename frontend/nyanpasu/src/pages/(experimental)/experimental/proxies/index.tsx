import { AppContentScrollArea } from '@/components/ui/scroll-area'
import useIsMobile from '@/hooks/use-is-moblie'
import { useClashProxies } from '@nyanpasu/interface'
import { createFileRoute, Navigate, useMatches } from '@tanstack/react-router'
import ProxiesNavigate from './_modules/proxies-navigate'

export const Route = createFileRoute('/(experimental)/experimental/proxies/')({
  component: RouteComponent,
})

function RouteComponent() {
  const matches = useMatches()

  const isMobile = useIsMobile()

  const currentRoute = matches[matches.length - 1]

  const { data: proxies } = useClashProxies()

  const fristGroup = proxies?.groups[0].name

  if (currentRoute?.id === Route.id && !isMobile && fristGroup) {
    return (
      <Navigate
        to="/experimental/proxies/group/$name"
        params={{
          name: fristGroup,
        }}
      />
    )
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
