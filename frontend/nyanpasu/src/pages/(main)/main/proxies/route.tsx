import BoxOutlineRounded from '~icons/material-symbols/box-outline-rounded'
import DirectionsRunRounded from '~icons/material-symbols/directions-run-rounded'
import { z } from 'zod'
import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { Button } from '@/components/ui/button'
import { AppContentScrollArea } from '@/components/ui/scroll-area'
import { Sidebar, SidebarContent } from '@/components/ui/sidebar'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { ProxyMode, useClashProxies, useProxyMode } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
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

const Empty = () => {
  const proxyMode = useProxyMode()

  const Icon = proxyMode.value.direct ? DirectionsRunRounded : BoxOutlineRounded

  const handleSwitchMode = useLockFn(async (mode: ProxyMode) => {
    await proxyMode.upsert(mode)
  })

  return (
    <div
      className="absolute inset-0 flex flex-col items-center justify-center gap-4"
      data-slot="proxies-no-proxies"
    >
      <Icon className="text-surface-variant size-16" />

      <p
        className="text-surface-variant text-sm"
        data-slot="proxies-no-proxies-message"
      >
        {proxyMode.value.direct
          ? m.proxies_group_direct_message()
          : m.proxies_group_empty_message()}
      </p>

      {proxyMode.value.direct ? (
        <div className="flex items-center gap-2">
          <Button
            variant="raised"
            data-slot="switch-rule-mode-button"
            onClick={() => handleSwitchMode('rule')}
          >
            {m.proxies_group_direct_switch_rule_button_text()}
          </Button>

          <Button
            variant="raised"
            data-slot="switch-script-mode-button"
            onClick={() => handleSwitchMode('global')}
          >
            {m.proxies_group_direct_switch_global_button_text()}
          </Button>
        </div>
      ) : (
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
      )}
    </div>
  )
}

function RouteComponent() {
  const {
    proxies: { data: proxies },
  } = useClashProxies()

  const isNoProxies = !proxies?.groups?.length || proxies?.groups?.length === 0

  const { value: proxyMode } = useProxyMode()

  return (
    <Sidebar data-slot="proxies-container">
      {!isNoProxies && (proxyMode.rule || proxyMode.script) && (
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
        {isNoProxies || proxyMode.direct ? (
          <Empty />
        ) : (
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
        )}
      </AppContentScrollArea>
    </Sidebar>
  )
}
