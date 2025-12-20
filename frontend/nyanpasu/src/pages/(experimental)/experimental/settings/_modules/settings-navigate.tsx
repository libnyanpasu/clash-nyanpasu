import ComputerOutlineRounded from '~icons/material-symbols/computer-outline-rounded'
import DisplayExternalInput from '~icons/material-symbols/display-external-input-rounded'
import FrameBugOutlineRounded from '~icons/material-symbols/frame-bug-outline-rounded'
import ListsRounded from '~icons/material-symbols/lists-rounded'
import NetworkNode from '~icons/material-symbols/network-node'
import SettingsBoltRounded from '~icons/material-symbols/settings-b-roll-rounded'
import SettingsEthernet from '~icons/material-symbols/settings-ethernet-rounded'
import ShareWindows from '~icons/material-symbols/share-windows-rounded'
import ViewQuilt from '~icons/material-symbols/view-quilt-rounded'
import ClashMeta from '@/assets/image/core/clash.meta.png'
import LogoSvg from '@/assets/image/logo.svg?react'
import { Button } from '@/components/ui/button'
import { cn } from '@nyanpasu/ui'
import { Link, useLocation } from '@tanstack/react-router'

const StyleClashMeta = () => {
  return (
    <img src={ClashMeta} alt="Clash Meta" className="size-8 grayscale-50" />
  )
}

const StyleLogo = () => {
  return (
    <LogoSvg className="[&_#element]:fill-primary [&_#bg]:fill-surface size-8" />
  )
}

const ROUTES = [
  {
    label: 'System Proxy',
    description: 'Configure the system proxy',
    href: '/experimental/settings/system-proxy',
    icon: SettingsEthernet,
  },
  {
    label: 'User Interface',
    description: 'Configure the user interface',
    href: '/experimental/settings/user-interface',
    icon: ViewQuilt,
  },
  {
    label: 'Clash Settings',
    description: 'Configure the clash settings',
    href: '/experimental/settings/clash-settings',
    icon: SettingsBoltRounded,
  },
  {
    label: 'Clash External Controll',
    description: 'Configure the clash external controll',
    href: '/experimental/settings/clash-external-controll',
    icon: DisplayExternalInput,
  },
  {
    label: 'Web UI',
    description: 'Configure the web ui',
    href: '/experimental/settings/web-ui',
    icon: ShareWindows,
  },
  {
    label: 'Clash Core',
    description: 'Configure the clash core',
    href: '/experimental/settings/clash-core',
    icon: StyleClashMeta,
  },
  {
    label: 'Clash Filed',
    description: 'Configure the clash filed',
    href: '/experimental/settings/clash-filed',
    icon: ListsRounded,
  },
  {
    label: 'System Behavior',
    description: 'Configure the system behavior',
    href: '/experimental/settings/system-behavior',
    icon: ComputerOutlineRounded,
  },
  {
    label: 'System Service',
    description: 'Configure the system service',
    href: '/experimental/settings/system-service',
    icon: NetworkNode,
  },
  {
    label: 'Nyanpasu Config',
    description: 'Configure the nyanpasu config',
    href: '/experimental/settings/nyanpasu-config',
    icon: StyleLogo,
  },
  {
    label: 'Debug Utils',
    description: 'Configure the debug utils',
    href: '/experimental/settings/debug-utils',
    icon: FrameBugOutlineRounded,
  },
  {
    label: 'About',
    description: 'About the nyanpasu',
    href: '/experimental/settings/about',
    icon: StyleLogo,
  },
] as const

export default function SettingsNavigate() {
  const location = useLocation()

  return (
    <div className="flex flex-col gap-2 p-2">
      {ROUTES.map((route) => (
        <Button
          key={route.href}
          variant="fab"
          data-active={String(location.pathname === route.href)}
          asChild
        >
          <Link
            className={cn(
              'h-16',
              'flex items-center gap-2',
              'data-[active=true]:bg-surface-variant/80',
              'data-[active=false]:bg-transparent',
              'data-[active=false]:shadow-none',
              'data-[active=false]:hover:shadow-none',
              'data-[active=false]:hover:bg-surface-variant/30',
            )}
            to={route.href}
          >
            <div className="flex items-center gap-2.5">
              <div className="size-8">
                <route.icon className="size-8" />
              </div>

              <div className="flex flex-col gap-1">
                <div className="text-sm font-medium">{route.label}</div>
                <div className="text-xs text-zinc-500">{route.description}</div>
              </div>
            </div>
          </Link>
        </Button>
      ))}
    </div>
  )
}
