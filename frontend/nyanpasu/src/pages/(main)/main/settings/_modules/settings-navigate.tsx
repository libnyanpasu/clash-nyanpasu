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
import { cva, type VariantProps } from 'class-variance-authority'
import { ComponentProps, ReactNode, useId } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { MaterialShape } from '@nyanpasu/ui/material-shape'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import LogoSvg from '@/assets/image/logo.svg?react'
import { m } from '@/paraglide/messages'
import { useCoreAgentSkills, useSetting } from '@nyanpasu/query'
import { cn } from '@nyanpasu/utils'
import packageJson from '@root/package.json'
import { Link, useMatchRoute } from '@tanstack/react-router'

const settingsNavigationGroupItemsVariants = cva([
  'flex flex-col gap-0.5',
  // Only the ends of a group get the large radius, like `SettingsGroup`.
  '[&>*:first-child:not(:only-child)]:rounded-b-sm',
  '[&>*:last-child:not(:only-child)]:rounded-t-sm',
  '[&>*:not(:first-child):not(:last-child)]:rounded-sm',
])

const settingsNavigationItemVariants = cva([
  'h-auto min-h-16 w-full justify-start rounded-3xl px-4 py-2.5',
  'flex items-center gap-3 text-left',
  'bg-surface text-on-surface dark:bg-surface dark:text-on-surface',
  // A state layer over the item's own fill instead of the button's hover fill.
  'data-[active=false]:hover:bg-surface',
  'hover:shadow-[inset_0_0_0_100px] hover:shadow-on-surface/8',
  'focus-visible:ring-primary focus-visible:ring-2 focus-visible:ring-inset',
  'transition-[border-radius,background-color,box-shadow] duration-300 ease-[cubic-bezier(0.2,0,0,1)]',
  'motion-reduce:transition-none',
  // The selected item rounds every corner, wherever it sits in its group.
  'data-[active=true]:bg-secondary-container data-[active=true]:hover:bg-secondary-container',
  'data-[active=true]:text-on-secondary-container data-[active=true]:rounded-3xl!',
])

const settingsNavigationItemIconVariants = cva(
  [
    'size-8 shrink-0 transition-colors duration-200',
    '[&>svg]:size-5',
    'group-data-[active=true]/settings-navigation-item:bg-primary',
    'group-data-[active=true]/settings-navigation-item:text-on-primary',
  ],
  {
    variants: {
      color: {
        red: 'bg-red-container text-on-red-container',
        orange: 'bg-orange-container text-on-orange-container',
        yellow: 'bg-yellow-container text-on-yellow-container',
        green: 'bg-green-container text-on-green-container',
        cyan: 'bg-cyan-container text-on-cyan-container',
        blue: 'bg-blue-container text-on-blue-container',
        purple: 'bg-purple-container text-on-purple-container',
        pink: 'bg-pink-container text-on-pink-container',
        neutral: 'bg-surface-variant text-on-surface-variant',
      },
    },
  },
)

type SettingsNavigationItemColor = NonNullable<
  VariantProps<typeof settingsNavigationItemIconVariants>['color']
>

const NyanpasuLogo = () => {
  return (
    <LogoSvg
      className={cn(
        'h-5 w-4 [&_#element]:fill-current',
        // The face must cover the silhouette, or the eyes and mouth disappear
        // into it; it takes the icon's own fill.
        '[&_#bg]:fill-surface-variant group-data-[active=true]/settings-navigation-item:[&_#bg]:fill-primary',
        // The cat's mass sits left of and below its box center (thin ears and
        // tail; its silhouette's centroid is at 44%, 58%), so a geometric
        // center looks off. Shift it toward the centroid's mirror, a little
        // less vertically because the outlined face reads lighter than a fill.
        'translate-x-[6%] -translate-y-[6%]',
      )}
    />
  )
}

function SettingsNavigationGroup({
  label,
  children,
}: {
  label: string
  children: ReactNode
}) {
  const labelId = useId()

  return (
    <section className="flex flex-col" data-slot="settings-navigation-group">
      <h2
        id={labelId}
        className="text-primary px-4 py-2 text-sm font-medium"
        data-slot="settings-navigation-group-label"
      >
        {label}
      </h2>

      <div
        role="group"
        aria-labelledby={labelId}
        className={settingsNavigationGroupItemsVariants()}
      >
        {children}
      </div>
    </section>
  )
}

function SettingsNavigationItem({
  icon,
  color,
  label,
  description,
  className,
  ...props
}: ComponentProps<typeof Link> & {
  icon: ReactNode
  color: SettingsNavigationItemColor
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
    <Button
      className={cn(
        'group/settings-navigation-item',
        settingsNavigationItemVariants(),
        className,
      )}
      data-active={String(isActive)}
      data-slot="settings-navigation-item"
      asChild
    >
      <Link aria-current={isActive ? 'page' : undefined} {...props}>
        <MaterialShape
          className={settingsNavigationItemIconVariants({ color })}
          // A circle at rest; the selected item morphs it into a cookie.
          depth={isActive ? undefined : 0}
          aria-hidden
        >
          {icon}
        </MaterialShape>

        <span className="flex min-w-0 flex-1 flex-col gap-0.5">
          <span className="text-sm leading-5 font-medium">{label}</span>

          <TextMarquee className="text-on-surface-variant group-data-[active=true]/settings-navigation-item:text-on-secondary-container/80 text-xs leading-4">
            {description}
          </TextMarquee>
        </span>
      </Link>
    </Button>
  )
}

export default function SettingsNavigate() {
  const { value: currentCore } = useSetting('core')
  const { data: agentSkills } = useCoreAgentSkills(currentCore)

  // The AI page hands agents the skills the current core ships.
  const hasAgentSkills = !!agentSkills?.length

  return (
    <nav
      aria-label={m.settings_navigation()}
      className="flex flex-col gap-4 p-3"
      data-slot="settings-navigation"
    >
      <SettingsNavigationGroup label={m.settings_group_system()}>
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
      </SettingsNavigationGroup>

      <SettingsNavigationGroup label={m.settings_group_clash()}>
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
      </SettingsNavigationGroup>

      <SettingsNavigationGroup label={m.settings_group_nyanpasu()}>
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
      </SettingsNavigationGroup>

      <SettingsNavigationGroup label={m.settings_group_about()}>
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
      </SettingsNavigationGroup>
    </nav>
  )
}
