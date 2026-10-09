import ChevronRightRounded from '~icons/material-symbols/chevron-right-rounded'
import MemoryOutlineRounded from '~icons/material-symbols/memory-outline-rounded'
import SettingsEthernetRounded from '~icons/material-symbols/settings-ethernet-rounded'
import { useMemo } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent } from '@nyanpasu/ui/card'
import { useDndGridContext } from '@nyanpasu/ui/dnd-grid'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import ServiceFallbackWarning from '@/components/settings/service-fallback-warning'
import {
  SystemProxyButton,
  TunModeButton,
} from '@/components/settings/system-proxy'
import useCoreIcon from '@/hooks/use-core-icon'
import { m } from '@/paraglide/messages'
import {
  ClashCores,
  useClashConfig,
  useClashCoreVersion,
  useClashSetting,
  useCoreStatus,
  useSetting,
  useSystemProxy,
  useSystemService,
} from '@nyanpasu/query'
import { cn } from '@nyanpasu/utils'
import { Link } from '@tanstack/react-router'
import { WidgetComponentProps } from './consts'
import { useWidgetConfig } from './provider'
import { WidgetConfigs, WidgetId } from './widget-config'
import WidgetItem from './widget-item'
import { WidgetHeader, WidgetTitle } from './widget-ui'

enum ProxyStatus {
  SYSTEM = 'system',
  TUN = 'tun',
  OCCUPIED = 'occupied',
  DISABLED = 'disabled',
}

const ProxyTitleRow = () => {
  const { value: enableSystemProxy } = useSetting('enable_system_proxy')

  const { value: enableTunMode } = useClashSetting('enable_tun_mode')

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
    <WidgetHeader>
      <WidgetTitle icon={SettingsEthernetRounded}>
        {m.dashboard_widget_proxy_status()}
      </WidgetTitle>

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
    </WidgetHeader>
  )
}

export function ProxyShortcutsWidget({
  id,
  onCloseClick,
}: WidgetComponentProps) {
  const config = useWidgetConfig(id, WidgetId.ProxyShortcuts)
  const { sourceOnly } = useDndGridContext()
  const buttons =
    config.order === 'system-first' ? ['system', 'tun'] : ['tun', 'system']

  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.ProxyShortcuts}
      minW={3}
      minH={2}
      onCloseClick={onCloseClick}
    >
      <Card className="flex size-full flex-col justify-between">
        <ProxyTitleRow />

        <CardContent
          className="min-h-0 flex-1 flex-row items-stretch justify-center gap-2 overflow-hidden py-3"
          data-slot="proxy-shortcut-buttons"
        >
          {buttons
            .filter(
              (button) =>
                config.buttons === 'both' || config.buttons === button,
            )
            .map((button) =>
              button === 'system' ? (
                <SystemProxyButton
                  key={button}
                  presentation="cookie"
                  elastic={config.layout === 'flex'}
                  disabled={sourceOnly}
                />
              ) : (
                <TunModeButton
                  key={button}
                  presentation="cookie"
                  elastic={config.layout === 'flex'}
                  disabled={sourceOnly}
                />
              ),
            )}
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
        'flex h-6 max-w-full min-w-0 items-center rounded-full text-xs font-medium',
        'bg-secondary-container/50 text-on-secondary-container',
      )}
      data-slot="core-status-badge"
    >
      <TextMarquee className="px-2" fadeEdges fadeWidth={8}>
        {message}
      </TextMarquee>
    </div>
  )
}

const CurrentCoreCard = ({
  config,
}: {
  config: WidgetConfigs[WidgetId.CoreShortcuts]
}) => {
  const { value: currentCoreKey } = useSetting('core')

  const { data: currentVersion } = useClashCoreVersion(currentCoreKey)

  const currentCoreIcon = useCoreIcon(currentCoreKey)

  const currentCoreName = currentCoreKey && ClashCores[currentCoreKey]

  const { data: coreStatus } = useCoreStatus()

  const isRunning = coreStatus?.status === 'Running'

  const channel = coreStatus?.controller
    ? 'Http' in coreStatus.controller
      ? 'HTTP'
      : 'NamedPipe' in coreStatus.controller
        ? 'IPC · Named Pipe'
        : 'IPC · Unix Socket'
    : '—'

  return (
    <Button
      variant="raised"
      className={cn(
        'group grid h-auto min-w-0 flex-1 rounded-[20px] px-2.5 py-0 text-left',
        config.showChannel
          ? 'grid-rows-[minmax(3.5rem,1fr)_minmax(2rem,0.6fr)]'
          : 'grid-rows-1',
        'bg-surface-variant/30 text-on-surface hover:bg-surface-variant/50 shadow-none hover:shadow-none focus:shadow-none',
        'focus-visible:outline-primary focus-visible:outline-2 focus-visible:-outline-offset-2',
      )}
      data-running={String(isRunning)}
      data-slot="current-core-card"
      asChild
    >
      <Link to="/main/settings/clash">
        <div className="flex w-full min-w-0 shrink-0 items-center gap-3">
          <div
            className={cn(
              'bg-surface/60 grid shrink-0 place-items-center rounded-xl',
              'size-10',
            )}
          >
            <img
              src={currentCoreIcon}
              alt=""
              className="size-8 object-contain"
              data-slot="core-icon"
            />
          </div>

          <div className="min-w-0 flex-1" data-slot="core-info">
            <div
              className="truncate text-base leading-5 font-semibold"
              title={currentCoreName || undefined}
              data-slot="core-name"
            >
              <TextMarquee className="w-full">
                {currentCoreName || '—'}
              </TextMarquee>
            </div>
            {config.showVersion && (
              <div
                className="text-on-surface-variant truncate text-xs leading-4 font-normal"
                title={currentVersion}
                data-slot="core-version"
              >
                <TextMarquee className="w-full">
                  {currentVersion ?? '—'}
                </TextMarquee>
              </div>
            )}
          </div>

          {coreStatus && (
            <div
              className={cn(
                'flex shrink-0 items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium',
                isRunning
                  ? 'bg-primary-container text-on-primary-container'
                  : 'bg-surface-variant text-on-surface-variant',
              )}
              data-slot="core-status"
            >
              <span
                className="size-1.5 shrink-0 rounded-full bg-current"
                aria-hidden="true"
                data-slot="core-status-indicator"
              />
              <span data-slot="core-status-text">
                {isRunning
                  ? m.dashboard_widget_core_status_running()
                  : m.dashboard_widget_core_status_stopped()}
              </span>
            </div>
          )}
        </div>

        {config.showChannel && (
          <div
            className="border-outline-variant/40 text-on-surface-variant flex w-full min-w-0 items-center gap-2 border-t text-xs leading-4 font-normal"
            data-slot="core-control-channel"
          >
            <span className="min-w-0 truncate">
              {m.settings_clash_control_channel_label()}
            </span>
            <span className="text-on-surface ml-auto shrink-0 font-medium">
              {channel}
            </span>
            <ChevronRightRounded
              className="size-4 shrink-0"
              aria-hidden="true"
            />
          </div>
        )}
      </Link>
    </Button>
  )
}

export function CoreShortcutsWidget({
  id,
  onCloseClick,
}: WidgetComponentProps) {
  const config = useWidgetConfig(id, WidgetId.CoreShortcuts)

  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.CoreShortcuts}
      minW={4}
      minH={2}
      onCloseClick={onCloseClick}
    >
      <Card className="flex size-full flex-col justify-between">
        <WidgetHeader>
          <WidgetTitle icon={MemoryOutlineRounded}>
            {m.dashboard_widget_core_status()}
          </WidgetTitle>

          <ServiceFallbackWarning className="-ml-2" />

          <CoreStatusBadge />
        </WidgetHeader>

        <CardContent className="min-h-0 flex-1 pt-2 pb-3">
          <CurrentCoreCard config={config} />
        </CardContent>
      </Card>
    </WidgetItem>
  )
}
