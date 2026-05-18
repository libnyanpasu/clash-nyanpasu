import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { AppContentScrollArea } from '@/components/ui/scroll-area'
import { Sidebar, SidebarContent } from '@/components/ui/sidebar'
import { cn } from '@nyanpasu/utils'
import { createFileRoute } from '@tanstack/react-router'
import ProfilesNavigate from './_modules/profiles-navigate'

export const Route = createFileRoute('/(main)/main/profiles')({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <Sidebar data-slot="profiles-container">
      <SidebarContent
        className="bg-surface-variant/10"
        data-slot="profiles-sidebar-scroll-area"
      >
        <ProfilesNavigate className="p-2" />
      </SidebarContent>

      <AppContentScrollArea
        className={cn(
          'group/profiles-content flex-[3_1_auto]',
          // for AnimatedOutletPreset transition to work properly,
          // the scroll area must have overflow: clip
          'overflow-clip',
        )}
        data-slot="profiles-content-scroll-area"
      >
        <div
          className={cn(
            'container mx-auto w-full max-w-7xl',
            'flex min-h-full flex-col',
          )}
          data-slot="profiles-content"
        >
          <AnimatedOutletPreset className="flex flex-1 flex-col" />
        </div>
      </AppContentScrollArea>
    </Sidebar>
  )
}
