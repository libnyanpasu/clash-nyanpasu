import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { AppContentScrollArea } from '@/components/ui/scroll-area'
import { Sidebar, SidebarContent } from '@/components/ui/sidebar'
import { cn } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'
import ProfilesNavigate from './_modules/profiles-navigate'

export const Route = createFileRoute('/(experimental)/experimental/profiles')({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <Sidebar data-slot="profiles-container">
      <SidebarContent
        className="bg-surface-variant/10"
        data-slot="profiles-sidebar-scroll-area"
      >
        <ProfilesNavigate />
      </SidebarContent>

      <AppContentScrollArea
        className={cn(
          'group/profiles-content flex-[3_1_auto]',
          'overflow-clip',
        )}
        data-slot="profiles-content-scroll-area"
      >
        <div
          className={cn(
            'container mx-auto w-full min-w-full',
            'min-h-[calc(100vh-40px-64px)]',
            'sm:min-h-[calc(100vh-40px-48px)]',
          )}
          data-slot="profiles-content"
        >
          <AnimatedOutletPreset />
        </div>
      </AppContentScrollArea>
    </Sidebar>
  )
}
