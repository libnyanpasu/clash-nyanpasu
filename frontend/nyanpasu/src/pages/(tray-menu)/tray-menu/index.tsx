import DashboardIcon from '~icons/material-symbols/dashboard-rounded'
import NetworkPing from '~icons/material-symbols/network-ping-rounded'
import SettingsEthernet from '~icons/material-symbols/settings-ethernet-rounded'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@/components/ui/segmented-button'
import TextMarquee from '@/components/ui/text-marquee'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { useLockFn } from '@/hooks/use-lock-fn'
import { useSystemProxy, useTunMode } from '@/hooks/use-proxy-settings'
import { m } from '@/paraglide/messages'
import {
  commands,
  CopyEnvOption,
  ProxyMode as ProxyModeType,
  useProxyMode,
} from '@interface/ipc'
import { createFileRoute } from '@tanstack/react-router'
import { ActionButton } from './_modules/action-button'
import { useTrayClickHandler } from './_modules/hooks'

const isDev = import.meta.env.DEV

export const Route = createFileRoute('/(tray-menu)/tray-menu/')({
  component: RouteComponent,
})

const OpenDashboardButton = () => {
  const handleClick = useLockFn(async () => {
    await commands.createMainWindow()
  })

  return (
    <ActionButton className="col-span-2" onClick={handleClick}>
      <DashboardIcon />

      <span>{m.tray_menu_open_dashboard()}</span>
    </ActionButton>
  )
}

const SystemProxyButton = () => {
  const { execute, isActive } = useSystemProxy()

  return (
    <ActionButton
      className="h-14 flex-col items-start gap-0.5"
      checked={isActive}
      onClick={() => execute()}
    >
      <NetworkPing />

      <TextMarquee className="w-full min-w-0">
        {m.tray_menu_toggle_system_proxy({
          status: isActive ? m.common_enabled() : m.common_disabled(),
        })}
      </TextMarquee>
    </ActionButton>
  )
}

const TunModeButton = () => {
  const { execute, isActive } = useTunMode()

  return (
    <ActionButton
      className="h-14 flex-col items-start gap-0.5"
      checked={isActive}
      onClick={() => execute()}
    >
      <SettingsEthernet />

      <TextMarquee className="w-full min-w-0">
        {m.tray_menu_toggle_tun_mode({
          status: isActive ? m.common_enabled() : m.common_disabled(),
        })}
      </TextMarquee>
    </ActionButton>
  )
}

const ProxyMode = () => {
  const { value, upsert } = useProxyMode()

  const proxyModeMessages = {
    global: m.tray_menu_proxy_mode_global(),
    direct: m.tray_menu_proxy_mode_direct(),
    rule: m.tray_menu_proxy_mode_rule(),
    script: m.tray_menu_proxy_mode_script(),
  } satisfies Record<ProxyModeType, string>

  const handleModeChange = useTrayClickHandler(async (mode: ProxyModeType) => {
    await upsert(mode)
  })

  const selectedMode = Object.entries(value).find(([, enabled]) => enabled)?.[0]

  return (
    <SegmentedButton
      className="col-span-2 h-10"
      variant="tabs"
      value={selectedMode}
      onValueChange={(mode) => handleModeChange(mode as ProxyModeType)}
    >
      {Object.keys(value).map((mode) => (
        <SegmentedButtonItem
          key={mode}
          className="px-0 text-sm font-bold"
          value={mode}
        >
          {proxyModeMessages[mode as ProxyModeType]}
        </SegmentedButtonItem>
      ))}
    </SegmentedButton>
  )
}

const EnvGrid = () => {
  const handleClick = useLockFn(async (type: CopyEnvOption) => {
    await commands.copyClashEnv(type)
  })

  const messages = {
    shell: m.tray_menu_copy_env_shell(),
    cmd: m.tray_menu_copy_env_cmd(),
    pwsh: m.tray_menu_copy_env_pwsh(),
  } satisfies Record<CopyEnvOption, string>

  return (
    <div className="col-span-2 grid grid-cols-3 gap-3">
      {Object.entries(messages).map(([key, value]) => (
        <Tooltip key={key}>
          <TooltipTrigger asChild>
            <ActionButton
              key={key}
              onClick={() => handleClick(key as CopyEnvOption)}
            >
              {key}
            </ActionButton>
          </TooltipTrigger>

          <TooltipContent>{value}</TooltipContent>
        </Tooltip>
      ))}
    </div>
  )
}

function RouteComponent() {
  return (
    <div
      className="grid grid-cols-2 gap-3 p-3"
      data-tauri-drag-region={isDev ? true : undefined}
    >
      <OpenDashboardButton />

      <SystemProxyButton />

      <TunModeButton />

      <ProxyMode />

      <EnvGrid />
    </div>
  )
}
