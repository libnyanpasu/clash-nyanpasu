import { ComponentProps } from 'react'
import AnimatedLogo from '@/components/logo/animated-logo'
import { isMacOS } from '@/consts'
import useWindowMaximized from '@/hooks/use-window-maximized'
import { cn } from '@nyanpasu/ui'
import HeaderMenu from './header-menu'
import WindowControl from './window-control'

const APP_NAME = 'Clash Nyanpasu'

const Title = () => {
  return (
    <div
      className="flex items-center gap-2"
      data-slot="app-header-logo-container"
      data-tauri-drag-region
    >
      <AnimatedLogo className="size-5" />

      <div
        className="text-on-surface text-base font-bold text-nowrap"
        data-slot="app-header-logo-name"
        data-tauri-drag-region
      >
        {APP_NAME}
      </div>
    </div>
  )
}

export function DefaultHeader({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      className={cn(
        'dark:bg-primary-container bg-inverse-primary flex h-10 w-full items-center px-3',
        'justify-between',
        className,
      )}
      data-slot="app-header"
      data-tauri-drag-region
      {...props}
    >
      <div className="flex items-center gap-2" data-tauri-drag-region>
        <Title />
        <HeaderMenu className="hidden md:flex" />
      </div>

      <WindowControl />
    </div>
  )
}

export function MacOSHeader({ className, ...props }: ComponentProps<'div'>) {
  const { isMaximized } = useWindowMaximized()

  return (
    <div
      className={cn(
        'bg-primary-container relative flex h-10 w-full items-center px-3',
        'justify-center',
        className,
      )}
      data-slot="app-header"
      data-tauri-drag-region
      {...props}
    >
      <div
        className={cn(
          'absolute left-22 hidden items-center md:flex',
          isMaximized ? 'left-2' : 'left-22',
        )}
        data-tauri-drag-region
      >
        <HeaderMenu />
      </div>

      <Title />
    </div>
  )
}

export default function Header({ className, ...props }: ComponentProps<'div'>) {
  return isMacOS ? (
    <MacOSHeader className={className} {...props} />
  ) : (
    <DefaultHeader className={className} {...props} />
  )
}
