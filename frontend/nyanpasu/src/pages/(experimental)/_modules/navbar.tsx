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
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/ui'
import { Link, useLocation } from '@tanstack/react-router'

const ROUTES = [
  {
    label: m.navbar_label_dashboard(),
    href: '/experimental/dashboard',
    icon: DashboardRounded,
  },
  {
    label: m.navbar_label_proxies(),
    href: '/experimental/proxies',
    icon: Public,
  },
  {
    label: m.navbar_label_profiles(),
    href: '/experimental/profiles',
    icon: GridViewOutlineRounded,
  },
  {
    label: m.navbar_label_connections(),
    href: '/experimental/connections',
    icon: SettingsEthernetRounded,
  },
  {
    label: m.navbar_label_rules(),
    href: '/experimental/rules',
    icon: DesignServicesRounded,
  },
  {
    label: m.navbar_label_logs(),
    href: '/experimental/logs',
    icon: TerminalRounded,
  },
  {
    label: m.navbar_label_settings(),
    href: '/experimental/settings',
    icon: SettingsRounded,
  },
  {
    label: m.navbar_label_providers(),
    href: '/experimental/providers',
    icon: Apps,
  },
] as const

const NavbarButton = ({ className, ...props }: ButtonProps) => {
  return (
    <Button
      className={cn(
        'hover:bg-primary-container dark:hover:bg-primary-container min-w-0',
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
        'dark:bg-on-primary bg-primary-container flex items-center px-3',
        'h-16 sm:h-12',
        'justify-between sm:justify-start',
        'gap-2 lg:gap-1',
        className,
      )}
      data-slot="app-navbar"
      {...props}
    >
      {ROUTES.map((route) => (
        <Tooltip key={route.href}>
          <TooltipTrigger>
            <NavbarButton
              data-active={location.pathname.startsWith(route.href)}
              asChild
            >
              <Link
                className={cn(
                  'flex items-center justify-center gap-1',
                  'lg:w-fit lg:px-3',
                  'sm:h-8!',
                )}
                to={route.href}
              >
                <span className="size-5" data-slot="navbar-button-icon">
                  <route.icon className="size-5" />
                </span>

                <span
                  className="hidden lg:block"
                  data-slot="navbar-button-label"
                >
                  {route.label}
                </span>
              </Link>
            </NavbarButton>
          </TooltipTrigger>

          <TooltipContent
            side="bottom"
            sideOffset={-4}
            className="hidden sm:block md:hidden"
          >
            {route.label}
          </TooltipContent>
        </Tooltip>
      ))}
    </div>
  )
}
