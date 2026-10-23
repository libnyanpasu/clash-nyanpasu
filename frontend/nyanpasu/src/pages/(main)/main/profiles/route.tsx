import { cva } from 'class-variance-authority'
import { AppContentScrollArea } from '@nyanpasu/ui/scroll-area'
import { Sidebar, SidebarContent } from '@nyanpasu/ui/sidebar'
import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { useLeafRouteLayout } from '@/components/router/route-layout'
import { cn } from '@nyanpasu/utils'
import { createFileRoute } from '@tanstack/react-router'
import ProfilesDndProvider from './_modules/profiles-dnd-provider'
import ProfilesLayoutProvider from './_modules/profiles-layout-provider'
import ProfilesNavigate from './_modules/profiles-navigate'

export const Route = createFileRoute('/(main)/main/profiles')({
  component: RouteComponent,
})

// A scroll page grows with its content and the viewport scrolls. A fill page
// is exactly as tall as the viewport and uses its full width; its regions
// scroll internally.
const profilesContentVariants = cva('mx-auto flex w-full flex-col', {
  variants: {
    layout: {
      scroll: 'container max-w-7xl min-h-full',
      fill: 'h-full',
    },
  },
})

const profilesOutletVariants = cva('flex flex-1 flex-col', {
  variants: {
    layout: {
      scroll: '',
      fill: 'min-h-0',
    },
  },
})

function RouteComponent() {
  const layout = useLeafRouteLayout() ?? 'scroll'

  return (
    <ProfilesLayoutProvider>
      <ProfilesDndProvider>
        <Sidebar data-slot="profiles-container">
          <SidebarContent
            className="bg-surface-variant/50 min-w-72 dark:bg-transparent [&>div>div]:block!"
            data-slot="profiles-sidebar-scroll-area"
          >
            <ProfilesNavigate />
          </SidebarContent>

          <AppContentScrollArea
            className={cn(
              // A zero basis splits the width 2.5:1 with the sidebar regardless of
              // the routed content, so the sidebar keeps its width across routes.
              'group/profiles-content flex-[2.5_1_0%]',
              // for AnimatedOutletPreset transition to work properly,
              // the scroll area must have overflow: clip
              'overflow-clip',
              // A fill page scrolls in its own regions. Radix sets the viewport's
              // overflow inline, so only an important class overrides it.
              'data-[layout=fill]:[&>[data-slot=scroll-area-viewport]]:overflow-clip!',
              // The tool pages sit on the darker ground so their grouped rows
              // read as raised surfaces.
              'data-[layout=fill]:dark:bg-mixed-background',
            )}
            data-layout={layout}
            data-slot="profiles-content-scroll-area"
          >
            <div
              className={profilesContentVariants({ layout })}
              data-slot="profiles-content"
            >
              <AnimatedOutletPreset
                className={profilesOutletVariants({ layout })}
              />
            </div>
          </AppContentScrollArea>
        </Sidebar>
      </ProfilesDndProvider>
    </ProfilesLayoutProvider>
  )
}
