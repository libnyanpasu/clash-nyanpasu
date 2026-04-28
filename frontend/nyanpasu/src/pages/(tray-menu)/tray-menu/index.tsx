import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import DashboardIcon from '~icons/material-symbols/dashboard-rounded'
import ExitToAppRounded from '~icons/material-symbols/exit-to-app-rounded'
import FolderCode from '~icons/material-symbols/folder-code-rounded'
import MenuRounded from '~icons/material-symbols/menu-rounded'
import NetworkPing from '~icons/material-symbols/network-ping-rounded'
import Public from '~icons/material-symbols/public'
import RestartAltRounded from '~icons/material-symbols/restart-alt-rounded'
import SettingsEthernet from '~icons/material-symbols/settings-ethernet-rounded'
import TerminalIcon from '~icons/material-symbols/terminal-rounded'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@/components/ui/segmented-button'
import TextMarquee from '@/components/ui/text-marquee'
import { useLockFn } from '@/hooks/use-lock-fn'
import { useSystemProxy, useTunMode } from '@/hooks/use-proxy-settings'
import { m } from '@/paraglide/messages'
import {
  commands,
  CopyEnvOption,
  ProxyMode as ProxyModeType,
  useProxyMode,
} from '@interface/ipc'
import { createFileRoute, Link } from '@tanstack/react-router'
import { relaunch } from '@tauri-apps/plugin-process'
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

const ProxiesButton = () => {
  return (
    <ActionButton className="col-span-2" disableClose asChild>
      <Link to="/tray-menu/proxies">
        <Public />

        <span className="flex-1">{m.tray_menu_proxies()}</span>

        <ArrowForwardIosRounded />
      </Link>
    </ActionButton>
  )
}

const EnvCopyButton = () => {
  const handleClick = useTrayClickHandler(async (type: CopyEnvOption) => {
    await commands.copyClashEnv(type)
  })

  const messages = {
    shell: m.tray_menu_copy_env_shell(),
    cmd: m.tray_menu_copy_env_cmd(),
    pwsh: m.tray_menu_copy_env_pwsh(),
  } satisfies Record<CopyEnvOption, string>

  return (
    <DropdownMenu align="end">
      <DropdownMenuTrigger asChild>
        <ActionButton className="col-span-2" disableClose>
          <TerminalIcon />

          <span className="flex-1">{m.tray_menu_copy_env()}</span>

          <MenuRounded />
        </ActionButton>
      </DropdownMenuTrigger>

      <DropdownMenuContent className="rounded-2xl backdrop-blur">
        {Object.entries(messages).map(([key, value]) => (
          <DropdownMenuItem
            key={key}
            className="bg-surface-variant/30 h-10"
            onSelect={() => handleClick(key as CopyEnvOption)}
          >
            {value}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

const OpenDirectoryButton = () => {
  type DirectoryType = 'data' | 'config' | 'core' | 'log'

  const messages = {
    data: m.tray_menu_open_data_directory(),
    config: m.tray_menu_open_config_directory(),
    core: m.tray_menu_open_core_directory(),
    log: m.tray_menu_open_log_directory(),
  } satisfies Record<DirectoryType, string>

  const handleOpenDirectory = useTrayClickHandler(
    async (type: DirectoryType) => {
      switch (type) {
        case 'data':
          await commands.openAppDataDir()
          break
        case 'config':
          await commands.openAppConfigDir()
          break
        case 'core':
          await commands.openCoreDir()
          break
        case 'log':
          await commands.openLogsDir()
          break
      }
    },
  )

  return (
    <DropdownMenu align="end">
      <DropdownMenuTrigger asChild>
        <ActionButton className="col-span-2" disableClose>
          <FolderCode />

          <span className="flex-1">{m.tray_menu_open_directory()}</span>

          <MenuRounded />
        </ActionButton>
      </DropdownMenuTrigger>

      <DropdownMenuContent className="rounded-2xl backdrop-blur">
        {Object.entries(messages).map(([key, value]) => (
          <DropdownMenuItem
            key={key}
            className="bg-surface-variant/30 h-10"
            onSelect={() => handleOpenDirectory(key as DirectoryType)}
          >
            {value}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

const RestartButton = () => {
  type RestartType = 'app' | 'core'

  const messages = {
    app: m.tray_menu_restart_app(),
    core: m.tray_menu_restart_core(),
  } satisfies Record<RestartType, string>

  const handleRestart = useTrayClickHandler(async (type: RestartType) => {
    switch (type) {
      case 'app':
        await relaunch()
        break
      case 'core':
        await commands.restartSidecar()
        break
    }
  })

  return (
    <DropdownMenu align="end">
      <DropdownMenuTrigger asChild>
        <ActionButton className="col-span-2" disableClose>
          <RestartAltRounded />

          <span className="flex-1">{m.tray_menu_restart()}</span>

          <MenuRounded />
        </ActionButton>
      </DropdownMenuTrigger>

      <DropdownMenuContent className="rounded-2xl backdrop-blur">
        {Object.entries(messages).map(([key, value]) => (
          <DropdownMenuItem
            key={key}
            className="bg-surface-variant/30 h-10"
            onSelect={() => handleRestart(key as RestartType)}
          >
            {value}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

const QuitActionButton = () => {
  const handleClick = useLockFn(async () => {
    await commands.quitApplication()
  })

  return (
    <ActionButton className="col-span-2" onClick={handleClick}>
      <ExitToAppRounded />

      <span>{m.tray_menu_quit()}</span>
    </ActionButton>
  )
}

function RouteComponent() {
  return (
    <div
      className="grid h-dvh w-dvw grid-cols-2 gap-x-3 overflow-hidden p-3 pb-2"
      data-tauri-drag-region={isDev ? true : undefined}
    >
      <OpenDashboardButton />

      <SystemProxyButton />

      <TunModeButton />

      <ProxyMode />

      <ProxiesButton />

      <EnvCopyButton />

      <OpenDirectoryButton />

      <RestartButton />

      <QuitActionButton />
    </div>
  )
}
