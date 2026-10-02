import { AppContentScrollArea } from '@nyanpasu/ui/scroll-area'
import { Sidebar, SidebarContent } from '@nyanpasu/ui/sidebar'
import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
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
          // A zero basis splits the width 3:1 with the sidebar regardless of
          // the routed content, so the sidebar keeps its width across routes.
          'group/profiles-content flex-[3_1_0%]',
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
