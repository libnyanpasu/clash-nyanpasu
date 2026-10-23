import AutoAwesomeRounded from '~icons/material-symbols/auto-awesome-rounded'
import BugReportRounded from '~icons/material-symbols/bug-report-rounded'
import DataObjectRounded from '~icons/material-symbols/data-object-rounded'
import DeployedCodeRounded from '~icons/material-symbols/deployed-code-rounded'
import DesktopWindowsRounded from '~icons/material-symbols/desktop-windows-rounded'
import KeyboardRounded from '~icons/material-symbols/keyboard-rounded'
import LanRounded from '~icons/material-symbols/lan-rounded'
import LicenseRounded from '~icons/material-symbols/license-rounded'
import PaletteRounded from '~icons/material-symbols/palette-rounded'
import PowerSettingsNewRounded from '~icons/material-symbols/power-settings-new-rounded'
import ReceiptLongRounded from '~icons/material-symbols/receipt-long-rounded'
import SwapHorizRounded from '~icons/material-symbols/swap-horiz-rounded'
import SystemUpdateAltRounded from '~icons/material-symbols/system-update-alt-rounded'
import TuneRounded from '~icons/material-symbols/tune-rounded'
import VpnLockRounded from '~icons/material-symbols/vpn-lock-rounded'
import WidgetsRounded from '~icons/material-symbols/widgets-rounded'
import { ComponentProps, ReactNode } from 'react'
import {
  NavigationList,
  NavigationListGroup,
  NavigationListItem,
  NavigationListItemIcon,
  NavigationListItemText,
  type NavigationListItemColor,
} from '@nyanpasu/ui/navigation-list'
import LogoSvg from '@/assets/image/logo.svg?react'
import { m } from '@/paraglide/messages'
import { useCoreAgentSkills, useSetting } from '@nyanpasu/query'
import { cn } from '@nyanpasu/utils'
import packageJson from '@root/package.json'
import { Link, useMatchRoute } from '@tanstack/react-router'

const NyanpasuLogo = () => {
  return (
    <LogoSvg
      className={cn(
        'h-5 w-4 [&_#element]:fill-current',
        // The face must cover the silhouette, or the eyes and mouth disappear
        // into it; it takes the icon's own fill.
        '[&_#bg]:fill-surface-variant group-data-[active=true]/navigation-list-item:[&_#bg]:fill-primary',
        // The cat's mass sits left of and below its box center (thin ears and
        // tail; its silhouette's centroid is at 44%, 58%), so a geometric
        // center looks off. Shift it toward the centroid's mirror, a little
        // less vertically because the outlined face reads lighter than a fill.
        'translate-x-[6%] -translate-y-[6%]',
      )}
    />
  )
}

function SettingsNavigationItem({
  icon,
  color,
  label,
  description,
  ...props
}: ComponentProps<typeof Link> & {
  icon: ReactNode
  color: NavigationListItemColor
  label: string
  description: string
}) {
  const matchRoute = useMatchRoute()

  // An item stays selected on its child pages, unless the link asks for an
  // exact match because another item lives under its path.
  const isActive = !!matchRoute({
    to: props.to,
    fuzzy: !props.activeOptions?.exact,
  })

  return (
    <NavigationListItem active={isActive} asChild>
      <Link aria-current={isActive ? 'page' : undefined} {...props}>
        <NavigationListItemIcon color={color}>{icon}</NavigationListItemIcon>

        <NavigationListItemText headline={label} supporting={description} />
      </Link>
    </NavigationListItem>
  )
}

export default function SettingsNavigate() {
  const { value: currentCore } = useSetting('core')
  const { data: agentSkills } = useCoreAgentSkills(currentCore)

  // The AI page hands agents the skills the current core ships.
  const hasAgentSkills = !!agentSkills?.length

  return (
    <NavigationList aria-label={m.settings_navigation()}>
      <NavigationListGroup label={m.settings_group_system()}>
        <SettingsNavigationItem
          to="/main/settings/system/proxy"
          color="blue"
          icon={<VpnLockRounded />}
          label={m.settings_label_proxy()}
          description={m.settings_label_proxy_description()}
        />

        <SettingsNavigationItem
          to="/main/settings/system/service"
          color="green"
          icon={<PowerSettingsNewRounded />}
          label={m.settings_label_service()}
          description={m.settings_label_service_description()}
        />
      </NavigationListGroup>

      <NavigationListGroup label={m.settings_group_clash()}>
        <SettingsNavigationItem
          to="/main/settings/clash/core-manager"
          color="purple"
          icon={<DeployedCodeRounded />}
          label={m.settings_label_core_manager()}
          description={m.settings_label_core_manager_description()}
        />

        <SettingsNavigationItem
          to="/main/settings/clash/core"
          color="orange"
          icon={<TuneRounded />}
          label={m.settings_label_core_settings()}
          description={m.settings_label_core_settings_description()}
        />

        <SettingsNavigationItem
          to="/main/settings/clash/network"
          color="cyan"
          icon={<LanRounded />}
          label={m.settings_label_network()}
          description={m.settings_label_network_description()}
        />

        <SettingsNavigationItem
          to="/main/settings/clash/control"
          color="blue"
          icon={<SwapHorizRounded />}
          label={m.settings_label_control()}
          description={m.settings_label_control_description()}
        />

        <SettingsNavigationItem
          to="/main/settings/clash/runtime"
          color="yellow"
          icon={<DataObjectRounded />}
          label={m.settings_label_runtime()}
          description={m.settings_label_runtime_description()}
        />

        {hasAgentSkills && (
          <SettingsNavigationItem
            to="/main/settings/clash/ai"
            color="green"
            icon={<AutoAwesomeRounded />}
            label={m.settings_label_ai()}
            description={m.settings_label_ai_description()}
          />
        )}
      </NavigationListGroup>

      <NavigationListGroup label={m.settings_group_nyanpasu()}>
        <SettingsNavigationItem
          to="/main/settings/nyanpasu/interface"
          color="pink"
          icon={<PaletteRounded />}
          label={m.settings_label_user_interface()}
          description={m.settings_label_user_interface_description()}
        />

        <SettingsNavigationItem
          to="/main/settings/nyanpasu/tray"
          color="cyan"
          icon={<DesktopWindowsRounded />}
          label={m.settings_label_tray()}
          description={m.settings_label_tray_description()}
        />

        <SettingsNavigationItem
          to="/main/settings/nyanpasu/widget"
          color="green"
          icon={<WidgetsRounded />}
          label={m.settings_label_widget()}
          description={m.settings_label_widget_description()}
        />

        <SettingsNavigationItem
          to="/main/settings/nyanpasu/shortcuts"
          color="neutral"
          icon={<KeyboardRounded />}
          label={m.settings_nyanpasu_keyboard_shortcuts()}
          description={m.settings_label_shortcuts_description()}
        />

        <SettingsNavigationItem
          to="/main/settings/nyanpasu/logs"
          color="orange"
          icon={<ReceiptLongRounded />}
          label={m.settings_label_logs()}
          description={m.settings_label_logs_description()}
        />

        <SettingsNavigationItem
          to="/main/settings/nyanpasu/updates"
          color="purple"
          icon={<SystemUpdateAltRounded />}
          label={m.settings_label_updates()}
          description={m.settings_label_updates_description()}
        />

        <SettingsNavigationItem
          to="/main/settings/debug"
          color="red"
          icon={<BugReportRounded />}
          label={m.settings_label_debug()}
          description={m.settings_label_debug_description()}
        />
      </NavigationListGroup>

      <NavigationListGroup label={m.settings_group_about()}>
        <SettingsNavigationItem
          to="/main/settings/about"
          activeOptions={{ exact: true }}
          color="neutral"
          icon={<NyanpasuLogo />}
          label="Clash Nyanpasu"
          description={m.settings_label_about_version({
            version: packageJson.version,
          })}
        />

        <SettingsNavigationItem
          to="/main/settings/about/credits"
          color="yellow"
          icon={<LicenseRounded />}
          label={m.settings_label_licenses()}
          description={m.settings_label_licenses_description()}
        />
      </NavigationListGroup>
    </NavigationList>
  )
}
