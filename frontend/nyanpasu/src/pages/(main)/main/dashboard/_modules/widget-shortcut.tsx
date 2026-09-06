import { useMemo } from 'react'
import {
  SystemProxyButton,
  TunModeButton,
} from '@/components/settings/system-proxy'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader } from '@/components/ui/card'
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
import { cn } from '@nyanpasu/utils'
import { Link } from '@tanstack/react-router'
import { WidgetComponentProps } from './consts'
import WidgetItem from './widget-item'

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

export function ProxyShortcutsWidget({
  id,
  onCloseClick,
}: WidgetComponentProps) {
  return (
    <WidgetItem id={id} minW={3} minH={2} onCloseClick={onCloseClick}>
      <Card className="flex size-full flex-col justify-between">
        <ProxyTitleRow />

        <CardContent className="flex-1 gap-3">
          <SystemProxyButton className="h-full rounded-3xl" />

          <TunModeButton className="h-full rounded-3xl" />
        </CardContent>
      </Card>
    </WidgetItem>
  )
}

const CoreStatusBadge = () => {
  const {
    query: { data: serviceStatus },
  } = useSystemService()

  const { data: coreStatus } = useCoreStatus()

  const message = useMemo<string>(() => {
    // 两条查询都还没答复：此时落到下面的真值表会断言"内核已停 + 服务未安装"
    // 两件尚未确立的事实，冷启动时用户第一眼看到的就是这个假状态。
    if (!coreStatus || !serviceStatus) {
      return m.dashboard_widget_core_status_loading()
    }

    // 谁在跑内核由本进程的 RunType 决定，而不是 daemon 自报的 core_infos：
    // 服务模式关闭时残留的 daemon、或兼容门 fail-closed 后被降级成 normal 的
    // 会话，daemon 都可能仍在跑它自己的内核，那不是本 App 的那一个。
    const byService = coreStatus.type === 'service'

    // core is running, we check if it's running by service or by child process
    if (coreStatus.status === 'Running') {
      return byService
        ? m.dashboard_widget_core_status_running_by_service()
        : m.dashboard_widget_core_status_running_by_child_process()
    }

    let serviceMessage

    if (serviceStatus.status === 'running') {
      serviceMessage = m.dashboard_widget_core_service_running()
    } else if (serviceStatus.status === 'stopped') {
      serviceMessage = m.dashboard_widget_core_service_stopped()
    } else {
      serviceMessage = m.dashboard_widget_core_service_not_installed()
    }

    let stopedMessage

    // 先取 service 上报的内核状态；server 为空时整条链短路成 undefined，
    // 后续访问都走这个局部变量，不再有非对称可选链。
    const serviceCoreState = serviceStatus.server?.core_infos.state

    // core is stopped, but we don't know why, so we check the core status
    const coreStopReason =
      coreStatus.status && typeof coreStatus.status === 'object'
        ? coreStatus.status.Stopped?.reason
        : undefined

    if (
      byService &&
      serviceStatus.status === 'running' &&
      serviceCoreState !== undefined &&
      serviceCoreState !== 'Running'
    ) {
      // service 明确报告了内核已停：这一分支优先于本地 core status。
      stopedMessage = serviceCoreState.Stopped
        ? m.dashboard_widget_core_stopped_by_service_with_message({
            message: serviceCoreState.Stopped,
          })
        : m.dashboard_widget_core_stopped_by_service_unknown()
    } else if (coreStopReason) {
      stopedMessage = m.dashboard_widget_core_stopped_with_message({
        message: coreStopReason,
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

export function CoreShortcutsWidget({
  id,
  onCloseClick,
}: WidgetComponentProps) {
  return (
    <WidgetItem id={id} minW={4} minH={2} onCloseClick={onCloseClick}>
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
    </WidgetItem>
  )
}
