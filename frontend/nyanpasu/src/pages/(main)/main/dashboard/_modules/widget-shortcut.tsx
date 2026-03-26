import { useMemo } from 'react'
import {
  SystemProxyButton,
  TunModeButton,
} from '@/components/settings/system-proxy'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader } from '@/components/ui/card'
import { DndGridItem } from '@/components/ui/dnd-grid'
import TextMarquee from '@/components/ui/text-marquee'
import useCoreIcon from '@/hooks/use-core-icon'
import { m } from '@/paraglide/messages'
import {
  useClashConfig,
  useClashCores,
  useCoreStatus,
  useSetting,
  useSystemProxy,
  useSystemService,
} from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { Link } from '@tanstack/react-router'
import { WidgetId } from './consts'

enum ProxyStatus {
  SYSTEM = 'system',
  TUN = 'tun',
  OCCUPIED = 'occupied',
  DISABLED = 'disabled',
}

const ProxyTitleRow = () => {
  const { value: enableSystemProxy } = useSetting('enable_system_proxy')

  const { value: enableTunMode } = useSetting('enable_tun_mode')

  const { data: systemProxyStatus } = useSystemProxy()

  const {
    query: { data: clashConfigs },
  } = useClashConfig()

  const status = useMemo<ProxyStatus>(() => {
    if (enableTunMode) {
      return ProxyStatus.TUN
    }

    if (enableSystemProxy) {
      if (systemProxyStatus?.enable) {
        const port = Number(systemProxyStatus.server.split(':')[1])

        if (port === clashConfigs?.['mixed-port']) {
          return ProxyStatus.SYSTEM
        }

        return ProxyStatus.OCCUPIED
      }
    }

    return ProxyStatus.DISABLED
  }, [enableSystemProxy, enableTunMode, systemProxyStatus, clashConfigs])

  const messages = {
    [ProxyStatus.SYSTEM]: m.dashboard_widget_proxy_status_success_system(),
    [ProxyStatus.TUN]: m.dashboard_widget_proxy_status_success_tun(),
    [ProxyStatus.OCCUPIED]: m.dashboard_widget_proxy_status_occupied(),
    [ProxyStatus.DISABLED]: m.dashboard_widget_proxy_status_disabled(),
  }

  return (
    <CardHeader className="flex items-center gap-3">
      <span className="shrink-0 font-bold">
        {m.dashboard_widget_proxy_status()}
      </span>

      <Button
        variant="raised"
        className={cn(
          'flex h-6 min-w-0 items-center px-0',
          status === ProxyStatus.DISABLED &&
            'bg-secondary-container hover:bg-on-secondary',
          status === ProxyStatus.OCCUPIED &&
            'bg-error-container hover:bg-on-error',
          status === ProxyStatus.SYSTEM &&
            'bg-primary-container hover:bg-on-primary',
          status === ProxyStatus.TUN &&
            'bg-tertiary-container hover:bg-on-tertiary',
        )}
        asChild
      >
        <Link to="/main/settings/system">
          <TextMarquee className="px-2" fadeEdges fadeWidth={8}>
            {messages[status]}
          </TextMarquee>
        </Link>
      </Button>
    </CardHeader>
  )
}

export function ProxyShortcutsWidget() {
  return (
    <DndGridItem id={WidgetId.ProxyShortcuts} minW={3} minH={2}>
      <Card className="flex size-full flex-col justify-between">
        <ProxyTitleRow />

        <CardContent className="flex-1 gap-3">
          <SystemProxyButton className="h-full rounded-3xl" />

          <TunModeButton className="h-full rounded-3xl" />
        </CardContent>
      </Card>
    </DndGridItem>
  )
}

const CoreStatusBadge = () => {
  const {
    query: { data: serviceStatus },
  } = useSystemService()

  const { data: coreStatus } = useCoreStatus()

  const message = useMemo<string>(() => {
    // core is running, we check if it's running by service or by child process
    if (coreStatus?.status === 'Running') {
      if (serviceStatus?.server?.core_infos.state === 'Running') {
        return m.dashboard_widget_core_status_running_by_service()
      } else {
        return m.dashboard_widget_core_status_running_by_child_process()
      }
    }

    let stopedMessage
    let serviceMessage

    if (serviceStatus?.status === 'running') {
      serviceMessage = m.dashboard_widget_core_service_running()

      // service returned core status, but it's not running, so it's stopped by service
      if (
        serviceStatus?.server?.core_infos.state !== 'Running' &&
        serviceStatus?.server?.core_infos.state.Stopped
      ) {
        stopedMessage = m.dashboard_widget_core_stopped_by_service_with_message(
          {
            message: serviceStatus?.server.core_infos.state.Stopped,
          },
        )
      } else {
        stopedMessage = m.dashboard_widget_core_stopped_by_service_unknown()
      }
    }

    // service is not running, so core is either stopped by service or not installed
    if (serviceStatus?.status === 'stopped') {
      serviceMessage = m.dashboard_widget_core_service_stopped()
    } else {
      serviceMessage = m.dashboard_widget_core_service_not_installed()
    }

    // core is stopped, but we don't know why, so we check the core status
    if (coreStatus?.status.Stopped) {
      stopedMessage = m.dashboard_widget_core_stopped_with_message({
        message: coreStatus.status.Stopped,
      })
    } else {
      stopedMessage = m.dashboard_widget_core_stopped_unknown()
    }

    return `${stopedMessage} ${serviceMessage}`
  }, [serviceStatus, coreStatus])

  return (
    <div
      className={cn(
        'flex h-6 min-w-0 items-center rounded-full text-sm',
        'bg-surface-variant/50',
      )}
      data-slot="core-status-badge"
    >
      <TextMarquee className="px-2" fadeEdges fadeWidth={8}>
        {message}
      </TextMarquee>
    </div>
  )
}

const CurrentCoreCard = () => {
  const { query: clashCores } = useClashCores()

  const { value: currentCoreKey } = useSetting('clash_core')

  const currentCoreIcon = useCoreIcon(currentCoreKey)

  const currentCore = currentCoreKey && clashCores.data?.[currentCoreKey]

  const { data: coreStatus } = useCoreStatus()

  const isRunning = coreStatus?.status === 'Running'

  return (
    <Button
      variant="raised"
      className={cn(
        'group flex flex-1 items-center gap-4 rounded-2xl pr-3 pl-4',
        'bg-surface-variant/30 hover:bg-surface-variant',
      )}
      data-running={String(isRunning)}
      data-slot="current-core-card"
      asChild
    >
      <Link to="/main/settings/clash">
        <img
          src={currentCoreIcon}
          alt={currentCore?.name}
          className="size-12 shrink-0"
          data-slot="core-icon"
        />

        <div
          className="flex flex-1 flex-col items-start gap-1 truncate"
          data-slot="core-info"
        >
          <div className="font-semibold" data-slot="core-name">
            {currentCore?.name}
          </div>

          <div
            className="text-zinc-700 dark:text-zinc-300"
            data-slot="core-version"
          >
            {currentCore?.currentVersion}
          </div>
        </div>

        <div
          className="flex items-center gap-2 truncate pr-2"
          data-slot="core-status"
        >
          <div className="truncate" data-slot="core-status-text">
            {isRunning
              ? m.dashboard_widget_core_status_running()
              : m.dashboard_widget_core_status_stopped()}
          </div>

          <div
            className="relative flex size-3 shrink-0"
            data-slot="core-status-indicator"
          >
            <span
              className={cn(
                'absolute inline-flex size-full animate-ping rounded-full opacity-75',
                'group-data-[running=true]:bg-green-500',
                'group-data-[running=false]:opacity-0',
              )}
            />

            <span
              className={cn(
                'relative inline-flex size-full rounded-full',
                'group-data-[running=true]:bg-green-500',
                'group-data-[running=false]:bg-gray-400',
              )}
            />
          </div>
        </div>
      </Link>
    </Button>
  )
}

export function CoreShortcutsWidget() {
  return (
    <DndGridItem id={WidgetId.CoreShortcuts} minW={4} minH={2}>
      <Card className="flex size-full flex-col justify-between">
        <CardHeader>
          <span className="shrink-0 font-bold">
            {m.dashboard_widget_core_status()}
          </span>

          <CoreStatusBadge />
        </CardHeader>

        <CardContent className="flex-1">
          <CurrentCoreCard />
        </CardContent>
      </Card>
    </DndGridItem>
  )
}
