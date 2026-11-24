import ArrowDropDownRounded from '~icons/material-symbols/arrow-drop-down-rounded'
import ChevronRightRounded from '~icons/material-symbols/chevron-right-rounded'
import MemoryOutlineRounded from '~icons/material-symbols/memory-outline-rounded'
import NetworkPingRounded from '~icons/material-symbols/network-ping-rounded'
import SettingsEthernetRounded from '~icons/material-symbols/settings-ethernet-rounded'
import ShieldRounded from '~icons/material-symbols/shield-rounded'
import { Fragment, useMemo, type ReactNode } from 'react'
import { Button } from '@nyanpasu/ui/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { LauncherMenuChipButton } from '@nyanpasu/ui/launcher-menu'
import { ShapeToggle } from '@nyanpasu/ui/shape-toggle'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import ServiceFallbackWarning from '@/components/settings/service-fallback-warning'
import {
  SystemProxyButton,
  TransparentProxyButton,
  TunModeButton,
} from '@/components/settings/system-proxy'
import useCoreIcon from '@/hooks/use-core-icon'
import { useTransparentProxy } from '@/hooks/use-proxy-settings'
import { m } from '@/paraglide/messages'
import { PlatformLinux, useMatchesPlatform } from '@nyanpasu/platform/react'
import {
  ClashCores,
  useClashConfig,
  useClashCoreVersion,
  useClashSetting,
  useCoreStatus,
  useServiceStatus,
  useSetting,
  useSystemProxy,
} from '@nyanpasu/query'
import { cn } from '@nyanpasu/utils'
import { Link } from '@tanstack/react-router'
import {
  defineWidget,
  useWidgetConfig,
  useWidgetConfigActions,
  useWidgetInteractive,
  Widget,
  WidgetContent,
  WidgetControls,
  WidgetHeader,
  WidgetTitle,
} from './core'
import {
  DEFAULT_WIDGET_CONFIGS,
  WidgetId,
  type WidgetConfigs,
} from './widget-config'
import { ConfigChoice, ConfigToggle } from './widget-config-fields'
import { ConfigGroup, ConfigRow } from './widget-config-row'

enum ProxyStatus {
  SYSTEM = 'system',
  TUN = 'tun',
  TRANSPARENT = 'transparent',
  OCCUPIED = 'occupied',
  DISABLED = 'disabled',
}

const proxyShortcutButtons = [
  {
    value: 'system',
    label: m.dashboard_widget_proxy_shortcuts_config_system,
  },
  {
    value: 'tun',
    label: m.settings_system_proxy_tun_mode_label,
  },
] as const

const proxyStatusMessages = {
  [ProxyStatus.SYSTEM]: m.dashboard_widget_proxy_status_success_system,
  [ProxyStatus.TUN]: m.dashboard_widget_proxy_status_success_tun,
  [ProxyStatus.TRANSPARENT]: m.settings_clash_transparent_proxy_label,
  [ProxyStatus.OCCUPIED]: m.dashboard_widget_proxy_status_occupied,
  [ProxyStatus.DISABLED]: m.dashboard_widget_proxy_status_disabled,
}

function ProxyShortcutsFrame({
  header,
  content,
}: {
  header: ReactNode
  content: ReactNode
}) {
  return (
    <Widget scaling="proportional" data-widget-kind="proxy-shortcuts">
      {header}

      <WidgetContent
        className="flex-row items-stretch justify-center gap-[var(--w-gap-sm)] py-[var(--w-pad-y)]"
        data-slot="proxy-shortcut-buttons"
      >
        {content}
      </WidgetContent>
    </Widget>
  )
}

const ProxyTitleRow = () => {
  const { value: enableSystemProxy } = useSetting('enable_system_proxy')

  const { value: enableTunMode } = useClashSetting('enable_tun_mode')

  const transparentProxy = useTransparentProxy()

  const { data: systemProxyStatus } = useSystemProxy()

  const {
    query: { data: clashConfigs },
  } = useClashConfig()

  const interactive = useWidgetInteractive()

  const status = useMemo<ProxyStatus>(() => {
    if (enableTunMode) {
      return ProxyStatus.TUN
    }

    if (
      transparentProxy.visible &&
      transparentProxy.isActive &&
      transparentProxy.status?.active
    ) {
      return ProxyStatus.TRANSPARENT
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
  }, [
    enableSystemProxy,
    enableTunMode,
    systemProxyStatus,
    clashConfigs,
    transparentProxy.visible,
    transparentProxy.isActive,
    transparentProxy.status?.active,
  ])

  return (
    <WidgetHeader>
      <WidgetTitle />

      <WidgetControls>
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
            status === ProxyStatus.TRANSPARENT &&
              'bg-tertiary-container hover:bg-on-tertiary',
            status === ProxyStatus.TUN &&
              'bg-tertiary-container hover:bg-on-tertiary',
          )}
          asChild
        >
          <Link
            aria-disabled={!interactive}
            tabIndex={interactive ? 0 : -1}
            onClick={(event) => {
              if (!interactive) event.preventDefault()
            }}
            to="/main/settings/system/proxy"
          >
            <TextMarquee className="px-2" fadeEdges fadeWidth={8}>
              {proxyStatusMessages[status]()}
            </TextMarquee>
          </Link>
        </Button>
      </WidgetControls>
    </WidgetHeader>
  )
}

function ProxyShortcutsPreviewHeader() {
  return (
    <WidgetHeader>
      <WidgetTitle />

      <WidgetControls>
        <Button
          variant="raised"
          disabled
          className="bg-primary-container text-on-primary-container flex h-6 min-w-0 items-center px-2 text-xs"
        >
          {m.dashboard_widget_proxy_status_success_system()}
        </Button>
      </WidgetControls>
    </WidgetHeader>
  )
}

function ProxyShortcutsButtons({ preview }: { preview: boolean }) {
  const config = useWidgetConfig(WidgetId.ProxyShortcuts)
  const interactive = useWidgetInteractive()
  const buttons = [
    ...(config.order === 'system-first'
      ? ['system', 'tun']
      : ['tun', 'system']),
    'transparent',
  ]

  return (
    <>
      {buttons
        .filter((button) =>
          config.buttons.includes(button as 'system' | 'tun' | 'transparent'),
        )
        .map((button) => {
          const content = preview ? (
            <ShapeToggle
              disabled
              active={button === 'system'}
              elastic={config.layout === 'flex'}
              icon={
                button === 'system' ? (
                  <NetworkPingRounded />
                ) : button === 'tun' ? (
                  <SettingsEthernetRounded />
                ) : (
                  <ShieldRounded />
                )
              }
              label={
                button === 'system'
                  ? m.settings_system_proxy_system_proxy_label()
                  : button === 'tun'
                    ? m.settings_system_proxy_tun_mode_label()
                    : m.settings_clash_transparent_proxy_label()
              }
            />
          ) : button === 'system' ? (
            <SystemProxyButton
              presentation="cookie"
              elastic={config.layout === 'flex'}
              disabled={!interactive}
            />
          ) : button === 'tun' ? (
            <TunModeButton
              presentation="cookie"
              elastic={config.layout === 'flex'}
              disabled={!interactive}
            />
          ) : (
            <TransparentProxyButton
              elastic={config.layout === 'flex'}
              disabled={!interactive}
            />
          )
          return button === 'transparent' ? (
            <PlatformLinux key={button}>{content}</PlatformLinux>
          ) : (
            <Fragment key={button}>{content}</Fragment>
          )
        })}
    </>
  )
}

function ProxyShortcutsLive() {
  return (
    <ProxyShortcutsFrame
      header={<ProxyTitleRow />}
      content={<ProxyShortcutsButtons preview={false} />}
    />
  )
}

function ProxyShortcutsPreview() {
  return (
    <ProxyShortcutsFrame
      header={<ProxyShortcutsPreviewHeader />}
      content={<ProxyShortcutsButtons preview />}
    />
  )
}

const CoreStatusBadge = () => {
  const { data: serviceStatus } = useServiceStatus()

  const { data: coreStatus } = useCoreStatus()

  const message = useMemo<string>(() => {
    if (!coreStatus || !serviceStatus) {
      return m.dashboard_widget_core_status_loading()
    }

    const byService = coreStatus.type === 'service'

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

    const stoppedMessage =
      byService && serviceStatus.status === 'running'
        ? m.dashboard_widget_core_stopped_by_service_unknown()
        : m.dashboard_widget_core_stopped_unknown()

    return `${stoppedMessage} ${serviceMessage}`
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
    ? coreStatus.controller === 'Http'
      ? 'HTTP'
      : coreStatus.controller === 'NamedPipe'
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
      <Link to="/main/settings/clash/core-manager">
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
              className="flex min-w-0 items-center gap-2 text-base leading-5 font-semibold"
              title={currentCoreName || undefined}
              data-slot="core-name"
            >
              <TextMarquee className="min-w-0">
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

function CoreShortcutsPreviewCard({
  config,
}: {
  config: WidgetConfigs[WidgetId.CoreShortcuts]
}) {
  const previewCoreIcon = useCoreIcon('mihomo')

  return (
    <Button
      variant="raised"
      disabled
      className={cn(
        'grid h-full w-full min-w-0 rounded-[20px] px-2.5 py-0 text-left',
        config.showChannel
          ? 'grid-rows-[minmax(3.5rem,1fr)_minmax(2rem,0.6fr)]'
          : 'grid-rows-1',
        'bg-surface-variant/30 text-on-surface shadow-none',
      )}
      data-running="true"
      data-slot="current-core-card"
    >
      <div className="flex w-full min-w-0 items-center gap-3">
        <div className="bg-surface/60 grid size-10 shrink-0 place-items-center rounded-xl">
          <img
            src={previewCoreIcon}
            alt=""
            className="size-8 object-contain"
            data-slot="core-icon"
          />
        </div>
        <div className="min-w-0 flex-1" data-slot="core-info">
          <div
            className="truncate text-base leading-5 font-semibold"
            data-slot="core-name"
          >
            Mihomo
          </div>
          {config.showVersion && (
            <div
              className="text-on-surface-variant truncate text-xs leading-4 font-normal"
              data-slot="core-version"
            >
              v1.19.12
            </div>
          )}
        </div>
        <div
          className="bg-primary-container text-on-primary-container flex shrink-0 items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium"
          data-slot="core-status"
        >
          <span
            className="size-1.5 shrink-0 rounded-full bg-current"
            aria-hidden="true"
            data-slot="core-status-indicator"
          />
          <span data-slot="core-status-text">
            {m.dashboard_widget_core_status_running()}
          </span>
        </div>
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
            HTTP
          </span>
          <ChevronRightRounded className="size-4 shrink-0" aria-hidden="true" />
        </div>
      )}
    </Button>
  )
}

function CoreShortcutsFrame({
  header,
  content,
}: {
  header: ReactNode
  content: ReactNode
}) {
  return (
    <Widget scaling="proportional" data-widget-kind="core-shortcuts">
      <WidgetHeader>
        <WidgetTitle />

        {header}
      </WidgetHeader>

      <WidgetContent className="pt-2 pb-3">{content}</WidgetContent>
    </Widget>
  )
}

function CoreShortcutsLive() {
  const config = useWidgetConfig(WidgetId.CoreShortcuts)

  return (
    <CoreShortcutsFrame
      header={
        <>
          <ServiceFallbackWarning className="-ml-2" />
          <CoreStatusBadge />
        </>
      }
      content={<CurrentCoreCard config={config} />}
    />
  )
}

function CoreShortcutsPreview() {
  const config = useWidgetConfig(WidgetId.CoreShortcuts)

  return (
    <CoreShortcutsFrame
      header={
        <div
          className={cn(
            'flex h-6 max-w-full min-w-0 items-center rounded-full px-2 text-xs font-medium',
            'bg-primary-container text-on-primary-container',
          )}
          data-slot="core-status-badge"
        >
          {m.dashboard_widget_core_status_running_by_child_process()}
        </div>
      }
      content={<CoreShortcutsPreviewCard config={config} />}
    />
  )
}

function ProxyShortcutsConfigForm() {
  const showTransparent = useMatchesPlatform({
    surface: 'desktop',
    clientOs: 'linux',
  })
  const { config, disabled, updateConfig } = useWidgetConfigActions(
    WidgetId.ProxyShortcuts,
  )

  const visibleButtons = config.buttons.filter(
    (button) => button !== 'transparent' || showTransparent,
  )
  const buttonsLabel =
    visibleButtons.length === (showTransparent ? 3 : 2)
      ? m.dashboard_widget_proxy_shortcuts_config_both()
      : config.buttons
          .filter((button) => button !== 'transparent' || showTransparent)
          .map((button) =>
            button === 'system'
              ? m.dashboard_widget_proxy_shortcuts_config_system()
              : button === 'tun'
                ? m.settings_system_proxy_tun_mode_label()
                : m.settings_clash_transparent_proxy_label(),
          )
          .join(', ')

  return (
    <ConfigGroup>
      <ConfigChoice
        label={m.dashboard_widget_proxy_mode_config_layout()}
        value={config.layout}
        disabled={disabled}
        options={[
          {
            value: 'equal',
            label: m.dashboard_widget_proxy_shortcuts_config_equal(),
          },
          {
            value: 'flex',
            label: m.dashboard_widget_proxy_mode_config_flex(),
          },
        ]}
        onChange={(layout) => updateConfig({ layout })}
      />
      <ConfigRow label={m.dashboard_widget_proxy_shortcuts_config_buttons()}>
        <DropdownMenu align="end">
          <DropdownMenuTrigger asChild>
            <LauncherMenuChipButton
              disabled={disabled}
              aria-label={m.dashboard_widget_proxy_shortcuts_config_buttons()}
            >
              <span className="truncate">{buttonsLabel}</span>
              <ArrowDropDownRounded
                className="size-4 shrink-0"
                aria-hidden="true"
              />
            </LauncherMenuChipButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent>
            {(
              [
                ...proxyShortcutButtons,
                ...(showTransparent
                  ? [
                      {
                        value: 'transparent' as const,
                        label: m.settings_clash_transparent_proxy_label,
                      },
                    ]
                  : []),
              ] as const
            ).map(({ value, label }) => (
              <DropdownMenuCheckboxItem
                key={value}
                checked={config.buttons.includes(value)}
                disabled={
                  disabled ||
                  (config.buttons.includes(value) &&
                    config.buttons.filter(
                      (button) => button !== 'transparent' || showTransparent,
                    ).length === 1)
                }
                onSelect={(event) => event.preventDefault()}
                onCheckedChange={(checked) =>
                  updateConfig({
                    buttons: checked
                      ? [...config.buttons, value]
                      : config.buttons.filter((button) => button !== value),
                  })
                }
              >
                {label()}
              </DropdownMenuCheckboxItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
      </ConfigRow>
      {config.buttons.includes('system') && config.buttons.includes('tun') && (
        <ConfigChoice
          label={m.dashboard_widget_proxy_shortcuts_config_order()}
          value={config.order}
          disabled={disabled}
          options={[
            {
              value: 'system-first',
              label: m.dashboard_widget_proxy_shortcuts_config_system_first(),
            },
            {
              value: 'tun-first',
              label: m.dashboard_widget_proxy_shortcuts_config_tun_first(),
            },
          ]}
          onChange={(order) => updateConfig({ order })}
        />
      )}
    </ConfigGroup>
  )
}

function CoreShortcutsConfigForm() {
  const { config, disabled, updateConfig } = useWidgetConfigActions(
    WidgetId.CoreShortcuts,
  )

  return (
    <ConfigGroup>
      <ConfigToggle
        label={m.dashboard_widget_core_shortcuts_config_version()}
        checked={config.showVersion}
        disabled={disabled}
        onChange={(showVersion) => updateConfig({ showVersion })}
      />
      <ConfigToggle
        label={m.dashboard_widget_core_shortcuts_config_channel()}
        checked={config.showChannel}
        disabled={disabled}
        onChange={(showChannel) => updateConfig({ showChannel })}
      />
    </ConfigGroup>
  )
}

export const proxyShortcutsWidget = defineWidget({
  id: WidgetId.ProxyShortcuts,
  category: 'controls',
  title: m.dashboard_widget_proxy_status,
  icon: SettingsEthernetRounded,
  size: { minW: 3, minH: 2, defaultW: 3, defaultH: 2 },
  defaultConfig: DEFAULT_WIDGET_CONFIGS[WidgetId.ProxyShortcuts],
  Component: ProxyShortcutsLive,
  Preview: ProxyShortcutsPreview,
  ConfigForm: ProxyShortcutsConfigForm,
})

export const coreShortcutsWidget = defineWidget({
  id: WidgetId.CoreShortcuts,
  category: 'controls',
  title: m.dashboard_widget_core_status,
  icon: MemoryOutlineRounded,
  size: { minW: 4, minH: 2, defaultW: 4, defaultH: 2 },
  defaultConfig: DEFAULT_WIDGET_CONFIGS[WidgetId.CoreShortcuts],
  Component: CoreShortcutsLive,
  Preview: CoreShortcutsPreview,
  ConfigForm: CoreShortcutsConfigForm,
})
