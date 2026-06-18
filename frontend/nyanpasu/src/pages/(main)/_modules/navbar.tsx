import Apps from '~icons/material-symbols/apps'
import DashboardRounded from '~icons/material-symbols/dashboard-rounded'
import DesignServicesRounded from '~icons/material-symbols/design-services-rounded'
import GridViewOutlineRounded from '~icons/material-symbols/grid-view-outline-rounded'
import MenuRounded from '~icons/material-symbols/menu-rounded'
import Public from '~icons/material-symbols/public'
import SettingsEthernetRounded from '~icons/material-symbols/settings-ethernet-rounded'
import SettingsRounded from '~icons/material-symbols/settings-rounded'
import TerminalRounded from '~icons/material-symbols/terminal-rounded'
import { ComponentProps, useMemo } from 'react'
import AnimatedTabs, { AnimatedTabsItem } from '@/components/ui/animated-tabs'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import { useClashProxies } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import {
  Link,
  useMatchRoute,
  type LinkComponentProps,
  type RegisteredRouter,
} from '@tanstack/react-router'

function NavbarButton<
  const TFrom extends string = string,
  const TTo extends string | undefined = undefined,
  const TMaskFrom extends string = TFrom,
  const TMaskTo extends string = '',
>(
  props: LinkComponentProps<
    'a',
    RegisteredRouter,
    TFrom,
    TTo,
    TMaskFrom,
    TMaskTo
  >,
) {
  const matchRoute = useMatchRoute()

  const isActive = !!matchRoute({
    ...props,
    fuzzy: true,
  })

  return (
    <AnimatedTabsItem
      className={cn('[&_svg]:size-5')}
      data-active={String(isActive)}
      data-slot="animated-tabs-item"
      isActive={isActive}
      asChild
    >
      <Link {...props} />
    </AnimatedTabsItem>
  )
}

const NavbarLabel = ({ className, ...props }: ComponentProps<'span'>) => {
  return (
    <span
      className={cn('text-sm font-medium text-nowrap', className)}
      data-slot="navbar-label"
      {...props}
    />
  )
}

const MoblieNavbarContainer = ({
  className,
  ...props
}: ComponentProps<'div'>) => {
  return (
    <div
      className={cn(
        'flex flex-col items-center gap-1',
        'min-w-0 flex-1',
        // '**:data-[slot=animated-tabs-item]:py-1!',
        className,
      )}
      data-slot="mobile-navbar-container"
      {...props}
    />
  )
}

export const DefaultNavbar = () => {
  const {
    proxies: { data: proxies },
  } = useClashProxies()

  const fristGroup = useMemo(() => {
    return proxies?.groups[0]?.name
  }, [proxies])

  return (
    <AnimatedTabs
      className={cn(
        'bg-transparent!',
        '**:data-[slot=animated-tabs-indicator]:bg-inverse-primary',
        '**:dark:data-[slot=animated-tabs-indicator]:bg-primary-container',
      )}
      data-slot="app-navbar"
      variant="pill"
      size="sm"
    >
      <NavbarButton to="/main/dashboard">
        <DashboardRounded />

        <NavbarLabel>{m.navbar_label_dashboard()}</NavbarLabel>
      </NavbarButton>

      {fristGroup ? (
        <NavbarButton
          to="/main/proxies/group/$name"
          params={{ name: fristGroup }}
        >
          <Public />

          <NavbarLabel>{m.navbar_label_proxies()}</NavbarLabel>
        </NavbarButton>
      ) : (
        <NavbarButton to="/main/proxies">
          <Public />

          <NavbarLabel>{m.navbar_label_proxies()}</NavbarLabel>
        </NavbarButton>
      )}

      <NavbarButton
        to="/main/profiles/$type"
        params={{
          type: 'profile',
        }}
      >
        <GridViewOutlineRounded />

        <NavbarLabel>{m.navbar_label_profiles()}</NavbarLabel>
      </NavbarButton>

      <NavbarButton to="/main/connections">
        <SettingsEthernetRounded />

        <NavbarLabel>{m.navbar_label_connections()}</NavbarLabel>
      </NavbarButton>

      <NavbarButton to="/main/rules">
        <DesignServicesRounded />

        <NavbarLabel>{m.navbar_label_rules()}</NavbarLabel>
      </NavbarButton>

      <NavbarButton to="/main/logs">
        <TerminalRounded />

        <NavbarLabel>{m.navbar_label_logs()}</NavbarLabel>
      </NavbarButton>

      <NavbarButton to="/main/settings/system">
        <SettingsRounded />

        <NavbarLabel>{m.navbar_label_settings()}</NavbarLabel>
      </NavbarButton>

      <NavbarButton to="/main/providers">
        <Apps />

        <NavbarLabel>{m.navbar_label_providers()}</NavbarLabel>
      </NavbarButton>
    </AnimatedTabs>
  )
}

export const MobileNavbar = () => {
  return (
    <AnimatedTabs
      className={cn(
        'h-full w-full bg-transparent! py-2',
        '**:data-[slot=animated-tabs-indicator]:bg-inverse-primary',
        '**:dark:data-[slot=animated-tabs-indicator]:bg-on-primary',
      )}
      variant="pill"
      size="sm"
    >
      <MoblieNavbarContainer>
        <NavbarButton to="/main/dashboard">
          <DashboardRounded />
        </NavbarButton>

        {m.navbar_label_dashboard()}
      </MoblieNavbarContainer>

      <MoblieNavbarContainer>
        <NavbarButton to="/main/proxies">
          <Public />
        </NavbarButton>

        {m.navbar_label_proxies()}
      </MoblieNavbarContainer>

      <MoblieNavbarContainer>
        <NavbarButton to="/main/connections">
          <SettingsEthernetRounded />
        </NavbarButton>

        {m.navbar_label_connections()}
      </MoblieNavbarContainer>

      <MoblieNavbarContainer>
        <NavbarButton to="/main/settings/system">
          <SettingsRounded />
        </NavbarButton>

        {m.navbar_label_settings()}
      </MoblieNavbarContainer>

      <DropdownMenu>
        <MoblieNavbarContainer>
          <DropdownMenuTrigger asChild>
            <Button
              className="min-w-0 flex-1 bg-transparent! px-4"
              variant="flat"
            >
              <MenuRounded className="size-5" />
            </Button>
          </DropdownMenuTrigger>

          {m.navbar_label_more()}
        </MoblieNavbarContainer>

        <DropdownMenuContent>
          <DropdownMenuItem asChild>
            <Link
              to="/main/profiles/$type"
              params={{
                type: 'profile',
              }}
            >
              <GridViewOutlineRounded />
              <span>{m.navbar_label_profiles()}</span>
            </Link>
          </DropdownMenuItem>

          <DropdownMenuItem asChild>
            <Link to="/main/rules">
              <DesignServicesRounded />
              <span>{m.navbar_label_rules()}</span>
            </Link>
          </DropdownMenuItem>

          <DropdownMenuItem asChild>
            <Link to="/main/logs">
              <TerminalRounded />
              <span>{m.navbar_label_logs()}</span>
            </Link>
          </DropdownMenuItem>

          <DropdownMenuItem asChild>
            <Link to="/main/providers">
              <Apps />
              <span>{m.navbar_label_providers()}</span>
            </Link>
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </AnimatedTabs>
  )
}
