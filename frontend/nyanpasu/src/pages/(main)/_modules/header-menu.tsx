import { ComponentProps } from 'react'
import { useNyanpasuUpdate } from '@/components/providers/nyanpasu-update-provider'
import { Button, ButtonProps } from '@/components/ui/button'
import { Action as AboutAction } from '@/pages/(main)/main/settings/about/route'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/ui'
import { Link } from '@tanstack/react-router'
import HeaderFileAction from './header-file-action'
import HeaderHelpAction from './header-help-action'
import HeaderSettingsAction from './header-settings-action'

const MenuButton = ({ className, ...props }: ButtonProps) => {
  return (
    <Button
      className={cn(
        'hover:bg-primary-container dark:hover:bg-on-primary h-8 min-w-0 px-3',
        'data-[state=open]:bg-primary-container dark:data-[state=open]:bg-on-primary',
        className,
      )}
      {...props}
    />
  )
}

const UpdateButton = () => {
  const { hasNewVersion } = useNyanpasuUpdate()

  if (!hasNewVersion) {
    return null
  }

  return (
    <div className="relative">
      <MenuButton className="flex items-center justify-center" asChild>
        <Link
          to="/main/settings/about"
          search={{
            action: AboutAction.NEED_UPDATE,
          }}
        >
          {m.header_new_version()}
        </Link>
      </MenuButton>

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
      className={cn('flex items-center gap-0.5', className)}
      {...props}
      data-tauri-drag-region
    >
      <HeaderFileAction>
        <MenuButton>{m.header_file_action_title()}</MenuButton>
      </HeaderFileAction>

      <HeaderSettingsAction>
        <MenuButton>{m.header_settings_action_title()}</MenuButton>
      </HeaderSettingsAction>

      <HeaderHelpAction>
        <MenuButton>{m.header_help_action_title()}</MenuButton>
      </HeaderHelpAction>

      <UpdateButton />
    </div>
  )
}
