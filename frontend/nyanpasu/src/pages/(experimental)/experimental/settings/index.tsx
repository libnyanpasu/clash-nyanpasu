import { AppContentScrollArea } from '@/components/ui/scroll-area'
import useIsMobile from '@/hooks/use-is-moblie'
import { cn } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'
import SettingsNavigate from './_modules/settings-navigate'

export const Route = createFileRoute('/(experimental)/experimental/settings/')({
  component: RouteComponent,
})

function RouteComponent() {
  const isMobile = useIsMobile()

  if (!isMobile) {
    return null
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
