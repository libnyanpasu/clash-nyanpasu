import { ComponentProps } from 'react'
import WindowControl from '@/components/window/window-control'
import WindowHeader from '@/components/window/window-header'
import WindowTitle from '@/components/window/window-title'
import { isMacOS } from '@/consts'
import useWindowMaximized from '@/hooks/use-window-maximized'
import { cn } from '@nyanpasu/ui'
import HeaderMenu from './header-menu'

const APP_NAME = 'Clash Nyanpasu'

const Title = () => {
  return (
    <WindowTitle>
      <div
        className="text-on-surface text-base font-bold text-nowrap"
        data-slot="app-header-logo-name"
        data-tauri-drag-region
      >
        {APP_NAME}
      </div>
    </WindowTitle>
  )
}

export function DefaultHeader({ className, ...props }: ComponentProps<'div'>) {
  return (
    <WindowHeader
      className={cn('items-center justify-between px-3', className)}
      data-slot="app-header"
      {...props}
    >
      <div className="flex items-center gap-2" data-tauri-drag-region>
        <Title />
        <HeaderMenu className="hidden md:flex" />
      </div>

      <WindowControl />
    </WindowHeader>
  )
}

export function MacOSHeader({ className, ...props }: ComponentProps<'div'>) {
  const { isMaximized } = useWindowMaximized()

  return (
    <WindowHeader
      className={cn('items-center justify-center px-3', className)}
      data-slot="app-header"
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
    </WindowHeader>
  )
}

export default function Header({ className, ...props }: ComponentProps<'div'>) {
  return isMacOS ? (
    <MacOSHeader className={className} {...props} />
  ) : (
    <DefaultHeader className={className} {...props} />
  )
}
