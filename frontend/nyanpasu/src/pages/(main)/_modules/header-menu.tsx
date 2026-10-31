import { ComponentProps } from 'react'
import { Menubar, menubarTriggerVariants } from '@nyanpasu/ui/menubar'
import { useNyanpasuUpdate } from '@/components/providers/nyanpasu-update-provider'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import { Link } from '@tanstack/react-router'
import HeaderFileAction from './header-file-action'
import HeaderHelpAction from './header-help-action'
import HeaderSettingsAction from './header-settings-action'

const UpdateButton = () => {
  const { hasNewVersion, isReady } = useNyanpasuUpdate()

  if (!hasNewVersion) {
    return null
  }

  return (
    <div className="relative">
      <Link
        to="/main/settings/nyanpasu/updates"
        className={cn(menubarTriggerVariants(), 'min-h-8 py-1')}
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
      <Menubar loop className="gap-0.5 bg-transparent p-0">
        <HeaderFileAction />

        <HeaderSettingsAction />

        <HeaderHelpAction />
      </Menubar>

      <UpdateButton />
    </div>
  )
}
