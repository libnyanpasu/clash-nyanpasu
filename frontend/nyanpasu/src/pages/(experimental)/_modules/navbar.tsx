import Apps from '~icons/material-symbols/apps'
import DashboardRounded from '~icons/material-symbols/dashboard-rounded'
import DesignServicesRounded from '~icons/material-symbols/design-services-rounded'
import GridViewOutlineRounded from '~icons/material-symbols/grid-view-outline-rounded'
import Public from '~icons/material-symbols/public'
import SettingsEthernetRounded from '~icons/material-symbols/settings-ethernet-rounded'
import SettingsRounded from '~icons/material-symbols/settings-rounded'
import TerminalRounded from '~icons/material-symbols/terminal-rounded'
import { ComponentProps, ReactNode } from 'react'
import { Button } from '@/components/ui/button'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import useIsMobile from '@/hooks/use-is-moblie'
import { m } from '@/paraglide/messages'
import { useClashProxies } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { Link, useLocation } from '@tanstack/react-router'

const NavbarButton = ({
  icon,
  label,
  ...props
}: Omit<ComponentProps<typeof Link>, 'children'> & {
  icon: ReactNode
  label: string
}) => {
  const location = useLocation()

  const isActive = Boolean(props.to && location.pathname.startsWith(props.to))

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          className={cn(
            'flex items-center justify-center gap-1',
            'lg:w-fit lg:px-3',
            'sm:h-8!',
            'hover:bg-primary-container dark:hover:bg-primary-container min-w-0',
            'dark:data-[active=true]:bg-primary-container! data-[active=true]:bg-inverse-primary!',
          )}
          data-active={String(Boolean(isActive))}
          asChild
        >
          <Link {...props}>
            <span className="size-5" data-slot="navbar-button-icon">
              {icon}
            </span>

            <span className="hidden lg:block" data-slot="navbar-button-label">
              {label}
            </span>
          </Link>
        </Button>
      </TooltipTrigger>

      <TooltipContent
        side="bottom"
        sideOffset={-4}
        className="hidden sm:block md:hidden"
      >
        {label}
      </TooltipContent>
    </Tooltip>
  )
}

export default function Navbar({ className, ...props }: ComponentProps<'div'>) {
  const isMobile = useIsMobile()

  const { data: proxies } = useClashProxies()
  const fristGroup = proxies?.groups[0].name

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
      <NavbarButton
        to="/experimental/dashboard"
        icon={<DashboardRounded className="size-5" />}
        label={m.navbar_label_dashboard()}
      />

      {isMobile || !fristGroup ? (
        <NavbarButton
          to="/experimental/proxies"
          icon={<Public className="size-5" />}
          label={m.navbar_label_proxies()}
        />
      ) : (
        <NavbarButton
          to="/experimental/proxies/group/$name"
          params={{ name: fristGroup } as never}
          icon={<Public className="size-5" />}
          label={m.navbar_label_proxies()}
        />
      )}

      <NavbarButton
        to="/experimental/profiles"
        icon={<GridViewOutlineRounded className="size-5" />}
        label={m.navbar_label_profiles()}
      />

      <NavbarButton
        to="/experimental/connections"
        icon={<SettingsEthernetRounded className="size-5" />}
        label={m.navbar_label_connections()}
      />

      <NavbarButton
        to="/experimental/rules"
        icon={<DesignServicesRounded className="size-5" />}
        label={m.navbar_label_rules()}
      />

      <NavbarButton
        to="/experimental/logs"
        icon={<TerminalRounded className="size-5" />}
        label={m.navbar_label_logs()}
      />

      <NavbarButton
        to={
          isMobile
            ? '/experimental/settings'
            : '/experimental/settings/system-proxy'
        }
        icon={<SettingsRounded className="size-5" />}
        label={m.navbar_label_settings()}
      />

      <NavbarButton
        to="/experimental/providers"
        icon={<Apps className="size-5" />}
        label={m.navbar_label_providers()}
      />
    </div>
  )
}
