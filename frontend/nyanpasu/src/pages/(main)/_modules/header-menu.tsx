import { ComponentProps } from 'react'
import { Button, ButtonProps } from '@/components/ui/button'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/ui'
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

// TODO: implement menu items
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
    </div>
  )
}
