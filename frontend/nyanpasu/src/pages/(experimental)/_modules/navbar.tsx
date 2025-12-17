import Apps from '~icons/material-symbols/apps'
import DashboardRounded from '~icons/material-symbols/dashboard-rounded'
import DesignServicesRounded from '~icons/material-symbols/design-services-rounded'
import GridViewOutlineRounded from '~icons/material-symbols/grid-view-outline-rounded'
import Public from '~icons/material-symbols/public'
import SettingsEthernetRounded from '~icons/material-symbols/settings-ethernet-rounded'
import SettingsRounded from '~icons/material-symbols/settings-rounded'
import TerminalRounded from '~icons/material-symbols/terminal-rounded'
import { ComponentProps } from 'react'
import { Button, ButtonProps } from '@/components/ui/button'
import { cn } from '@nyanpasu/ui'
import { Link, useLocation } from '@tanstack/react-router'

const ROUTES = [
  {
    label: 'Dashboard',
    href: '/experimental/dashboard',
    icon: DashboardRounded,
  },
  {
    label: 'Proxies',
    href: '/experimental/proxies',
    icon: Public,
  },
  {
    label: 'Profiles',
    href: '/experimental/profiles',
    icon: GridViewOutlineRounded,
  },
  {
    label: 'Connections',
    href: '/experimental/connections',
    icon: SettingsEthernetRounded,
  },
  {
    label: 'Rules',
    href: '/experimental/rules',
    icon: DesignServicesRounded,
  },
  {
    label: 'Logs',
    href: '/experimental/logs',
    icon: TerminalRounded,
  },
  {
    label: 'Settings',
    href: '/experimental/settings',
    icon: SettingsRounded,
  },
  {
    label: 'Providers',
    href: '/experimental/providers',
    icon: Apps,
  },
] as const

const NavbarButton = ({ className, ...props }: ButtonProps) => {
  return (
    <Button
      className={cn(
        'hover:bg-primary-container dark:hover:bg-primary-container h-8 min-w-0 px-3',
        'dark:data-[active=true]:bg-primary-container! data-[active=true]:bg-inverse-primary!',
        className,
      )}
      {...props}
    />
  )
}

export default function Navbar({ className, ...props }: ComponentProps<'div'>) {
  const location = useLocation()

  return (
    <div
      className={cn(
        'dark:bg-on-primary bg-primary-container flex items-center gap-1 px-3',
        'h-16 sm:h-12',
        className,
      )}
      data-slot="app-navbar"
      {...props}
    >
      {ROUTES.map((route) => (
        <NavbarButton
          key={route.href}
          data-active={location.pathname === route.href}
          asChild
        >
          <Link
            className="flex items-center justify-center gap-1"
            to={route.href}
          >
            <route.icon className="size-5" />

            <span>{route.label}</span>
          </Link>
        </NavbarButton>
      ))}
    </div>
  )
}
