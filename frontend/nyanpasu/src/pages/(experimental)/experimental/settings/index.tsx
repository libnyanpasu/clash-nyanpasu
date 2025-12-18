import { AppContentScrollArea } from '@/components/ui/scroll-area'
import useIsMobile from '@/hooks/use-is-moblie'
import { cn } from '@nyanpasu/ui'
import { createFileRoute, Navigate, useMatches } from '@tanstack/react-router'
import SettingsNavigate from './_modules/settings-navigate'

export const Route = createFileRoute('/(experimental)/experimental/settings/')({
  component: RouteComponent,
})

function RouteComponent() {
  const matches = useMatches()

  const isMobile = useIsMobile()

  const currentRoute = matches[matches.length - 1]

  // if the current route is the settings route, redirect to the system proxy route
  // but only on sm breakpoint and above (when sidebar is visible)
  if (currentRoute?.id === Route.id && !isMobile) {
    return <Navigate to="/experimental/settings/system-proxy" />
  }

  return (
    <AppContentScrollArea
      className={cn('bg-surface z-50 w-full')}
      data-slot="settings-sidebar-scroll-area"
    >
      <SettingsNavigate />
    </AppContentScrollArea>
  )
}
