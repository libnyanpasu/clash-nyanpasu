import { z } from 'zod'
import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { AppContentScrollArea } from '@/components/ui/scroll-area'
import { Sidebar, SidebarContent } from '@/components/ui/sidebar'
import { cn } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'
import { zodSearchValidator } from '@tanstack/router-zod-adapter'
import ProxiesNavigate from './_modules/proxies-navigate'

const searchSchema = z.object({
  searchQuery: z.string().optional().nullable(),
})

export const Route = createFileRoute('/(experimental)/experimental/proxies')({
  component: RouteComponent,
  validateSearch: zodSearchValidator(searchSchema),
})

function RouteComponent() {
  return (
    <Sidebar data-slot="proxies-container">
      <SidebarContent
        className="bg-surface-variant/10"
        data-slot="proxies-sidebar-scroll-area"
      >
        <ProxiesNavigate />
      </SidebarContent>

      <AppContentScrollArea
        className={cn('group/proxies-content flex-[3_1_auto]', 'overflow-clip')}
        data-slot="proxies-content-scroll-area"
      >
        <div
          className={cn(
            'container mx-auto w-full min-w-full',
            'min-h-[calc(100vh-40px-64px)]',
            'sm:min-h-[calc(100vh-40px-48px)]',
          )}
          data-slot="proxies-content"
        >
          <AnimatedOutletPreset />
        </div>
      </AppContentScrollArea>
    </Sidebar>
  )
}
