import BoxOutlineRounded from '~icons/material-symbols/box-outline-rounded'
import { z } from 'zod'
import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { Button } from '@/components/ui/button'
import { AppContentScrollArea } from '@/components/ui/scroll-area'
import { Sidebar, SidebarContent } from '@/components/ui/sidebar'
import { m } from '@/paraglide/messages'
import { useClashProxies } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { createFileRoute, Link } from '@tanstack/react-router'
import { zodSearchValidator } from '@tanstack/router-zod-adapter'
import { ProfileType } from '../profiles/_modules/consts'
import ProxiesNavigate from './_modules/proxies-navigate'

const searchSchema = z.object({
  searchQuery: z.string().optional().nullable(),
})

export const Route = createFileRoute('/(main)/main/proxies')({
  component: RouteComponent,
  validateSearch: zodSearchValidator(searchSchema),
})

const NoProxies = () => {
  return (
    <div
      className="absolute inset-0 flex flex-col items-center justify-center gap-4"
      data-slot="proxies-no-proxies"
    >
      <BoxOutlineRounded className="text-surface-variant size-16" />

      <p
        className="text-surface-variant text-sm"
        data-slot="proxies-no-proxies-message"
      >
        {m.proxies_group_empty_message()}
      </p>

      <Button variant="raised" data-slot="proxies-no-proxies-button" asChild>
        <Link
          className="flex items-center gap-2"
          to="/main/profiles/$type"
          params={{
            type: ProfileType.Profile,
          }}
        >
          {m.proxies_group_empty_button_text()}
        </Link>
      </Button>
    </div>
  )
}

function RouteComponent() {
  const {
    proxies: { data: proxies },
  } = useClashProxies()

  const isNoProxies = !proxies?.groups?.length || proxies?.groups?.length === 0

  return (
    <Sidebar data-slot="proxies-container">
      {!isNoProxies && (
        <SidebarContent
          className="bg-surface-variant/10"
          data-slot="proxies-sidebar-scroll-area"
        >
          <ProxiesNavigate />
        </SidebarContent>
      )}

      <AppContentScrollArea
        className={cn('group/proxies-content flex-[3_1_auto]', 'overflow-clip')}
        data-slot="proxies-content-scroll-area"
      >
        {!isNoProxies ? (
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
        ) : (
          <NoProxies />
        )}
      </AppContentScrollArea>
    </Sidebar>
  )
}
