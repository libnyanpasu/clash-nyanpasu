import SystemUpdateAltRounded from '~icons/material-symbols/system-update-alt-rounded'
import { ComponentProps } from 'react'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuPillTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { Menubar, menubarTriggerVariants } from '@nyanpasu/ui/menubar'
import LogoSvg from '@/assets/image/logo.svg?react'
import { useNyanpasuUpdate } from '@/components/providers/nyanpasu-update-provider'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import { Link } from '@tanstack/react-router'
import HeaderFileAction, { HeaderFileItems } from './header-file-action'
import HeaderHelpAction, { HeaderHelpItems } from './header-help-action'
import {
  HeaderMenuPrimitivesProvider,
  LAUNCHER_PRIMITIVES,
} from './header-menu-primitives'
import HeaderSettingsAction, {
  HeaderSettingsItems,
} from './header-settings-action'

const APP_NAME = 'Clash Nyanpasu'

const UpdateButton = () => {
  const { hasNewVersion, isReady } = useNyanpasuUpdate()

  if (!hasNewVersion) {
    return null
  }

  return (
    <div className="relative">
      <Link
        to="/main/settings/nyanpasu/updates"
        className={cn(
          menubarTriggerVariants(),
          'text-on-primary-container hover:bg-on-primary-container/10 min-h-8 py-1',
        )}
      >
        {isReady
          ? m.settings_about_update_ready_header()
          : m.header_new_version()}
      </Link>

      <span className="bg-on-error-container absolute top-1 right-0.5 size-1.5 rounded-full" />

      <span
        className={cn(
          'absolute top-1 right-0.5 size-1.5 animate-ping rounded-full',
          'bg-on-error-container',
        )}
      />
    </div>
  )
}

/**
 * Compact A17-style titlebar menu anchored to the logo and app name when the
 * horizontal menubar is hidden on narrow viewports.
 */
export function HeaderTitleMenu({ className }: { className?: string }) {
  const { hasNewVersion, isReady } = useNyanpasuUpdate()

  return (
    <DropdownMenu variant="launcher" align="start">
      <DropdownMenuPillTrigger
        className={cn('text-on-primary-container px-2', className)}
      >
        <span
          className="flex min-w-0 items-center gap-2"
          data-slot="app-header-logo-container"
        >
          <LogoSvg
            className="logo-colorized size-5 shrink-0"
            data-slot="app-header-logo"
            aria-hidden="true"
          />

          <span
            className="truncate text-base font-bold text-nowrap"
            data-slot="app-header-logo-name"
          >
            {APP_NAME}
          </span>

          {hasNewVersion && (
            <span
              className="bg-on-error-container size-1.5 shrink-0 rounded-full"
              aria-hidden="true"
            />
          )}
        </span>
      </DropdownMenuPillTrigger>

      <DropdownMenuContent sideOffset={6}>
        <HeaderMenuPrimitivesProvider value={LAUNCHER_PRIMITIVES}>
          <DropdownMenuGroup>
            <HeaderFileItems />
          </DropdownMenuGroup>

          <DropdownMenuGroup>
            <HeaderSettingsItems />
          </DropdownMenuGroup>

          <DropdownMenuGroup>
            <HeaderHelpItems />
          </DropdownMenuGroup>

          {hasNewVersion && (
            <DropdownMenuGroup>
              <DropdownMenuItem asChild>
                <Link to="/main/settings/nyanpasu/updates">
                  <SystemUpdateAltRounded
                    className="size-5 shrink-0"
                    aria-hidden="true"
                  />
                  <span>
                    {isReady
                      ? m.settings_about_update_ready_header()
                      : m.header_new_version()}
                  </span>
                </Link>
              </DropdownMenuItem>
            </DropdownMenuGroup>
          )}
        </HeaderMenuPrimitivesProvider>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

export default function HeaderMenu({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      data-slot="header-menu"
      className={cn('flex items-center gap-0.5', className)}
      {...props}
      data-tauri-drag-region
    >
      <Menubar
        loop
        className="[&_[data-slot=menubar-trigger]]:text-on-primary-container [&_[data-slot=menubar-trigger]]:hover:bg-on-primary-container/10 [&_[data-slot=menubar-trigger]]:data-[state=open]:bg-on-primary-container/10 gap-0.5 bg-transparent p-0"
      >
        <HeaderFileAction />

        <HeaderSettingsAction />

        <HeaderHelpAction />
      </Menubar>

      <UpdateButton />
    </div>
  )
}
