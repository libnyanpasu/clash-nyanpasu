import { AppContentScrollArea } from '@nyanpasu/ui/scroll-area'
import { Sidebar, SidebarContent } from '@nyanpasu/ui/sidebar'
import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { cn } from '@nyanpasu/utils'
import { createFileRoute } from '@tanstack/react-router'
import SettingsNavigate from './_modules/settings-navigate'

export const Route = createFileRoute('/(main)/main/settings')({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <Sidebar data-slot="settings-container">
      <SidebarContent
        className="bg-surface-variant/50 min-w-72 dark:bg-transparent [&>div>div]:block!"
        data-slot="settings-sidebar-scroll-area"
      >
        <SettingsNavigate />
      </SidebarContent>

      <AppContentScrollArea
        className={cn(
          // A zero basis splits the width 2.5:1 with the sidebar regardless of
          // the routed content, so the sidebar keeps its width across routes.
          'group/settings-content flex-[2.5_1_0%]',
          // for AnimatedOutletPreset transition to work properly,
          // the scroll area must have overflow: clip
          'overflow-clip',
        )}
        data-slot="settings-content-scroll-area"
      >
        <div
          className={cn(
            'container mx-auto w-full max-w-7xl',
            'flex min-h-full flex-col [&>div]:flex [&>div]:flex-1 [&>div]:flex-col',
          )}
          data-slot="settings-content"
        >
          <AnimatedOutletPreset />
        </div>
      </AppContentScrollArea>
    </Sidebar>
  )
}
