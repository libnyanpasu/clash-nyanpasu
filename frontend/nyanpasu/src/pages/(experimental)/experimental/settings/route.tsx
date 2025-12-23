import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { AppContentScrollArea } from '@/components/ui/scroll-area'
import { Sidebar, SidebarContent } from '@/components/ui/sidebar'
import { cn } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'
import SettingsNavigate from './_modules/settings-navigate'

export const Route = createFileRoute('/(experimental)/experimental/settings')({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <Sidebar data-slot="settings-container">
      <SidebarContent
        className="bg-surface-variant/10"
        data-slot="settings-sidebar-scroll-area"
      >
        <SettingsNavigate />
      </SidebarContent>

      <AppContentScrollArea
        className={cn(
          'group/settings-content flex-[3_1_auto]',
          'overflow-clip',
        )}
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
    </Sidebar>
  )
}
