import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { AppContentScrollArea } from '@/components/ui/scroll-area'
import useIsMobile from '@/hooks/use-is-moblie'
import { cn } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'
import SettingsNavigate from './_modules/settings-navigate'

export const Route = createFileRoute('/(experimental)/experimental/settings')({
  component: RouteComponent,
})

function RouteComponent() {
  const isMobile = useIsMobile()

  return (
    <div className="flex" data-slot="settings-container">
      {!isMobile && (
        <AppContentScrollArea
          className={cn('bg-surface z-50 max-w-96 min-w-64')}
          data-slot="settings-sidebar-scroll-area"
        >
          <SettingsNavigate />
        </AppContentScrollArea>
      )}

      <AppContentScrollArea
        className={cn('group/settings-content bg-surface/80 flex-[3_1_auto]')}
        data-slot="settings-content-scroll-area"
      >
        <div
          className={cn(
            'container mx-auto w-full max-w-7xl',
            'min-h-[calc(100vh-40px-64px)]',
            'sm:min-h-[calc(100vh-40px-48px)]',
          )}
          data-slot="settings-content"
        >
          <AnimatedOutletPreset />
        </div>
      </AppContentScrollArea>
    </div>
  )
}
