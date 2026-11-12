import { ComponentProps } from 'react'
import useWindowMaximized from '@/hooks/use-window-maximized'
import { cn } from '@nyanpasu/utils'
import WindowControl from './window-control'
import WindowHeader from './window-header'

export function DefaultHeader({
  className,
  children,
  beforeClose,
  ...props
}: ComponentProps<'div'> & {
  beforeClose?: ComponentProps<typeof WindowControl>['beforeClose']
}) {
  return (
    <WindowHeader
      className={cn('items-center justify-between px-3', className)}
      data-slot="app-header"
      {...props}
    >
      <div className="flex min-w-0 items-center gap-2" data-tauri-drag-region>
        {children}
      </div>

      <WindowControl beforeClose={beforeClose} />
    </WindowHeader>
  )
}

export function MacOSHeader({ className, ...props }: ComponentProps<'div'>) {
  return (
    <WindowHeader
      className={cn('items-center justify-center px-3', className)}
      data-slot="app-header-macos"
      {...props}
    />
  )
}

export function MacOSHeaderLeft({
  className,
  ...props
}: ComponentProps<'div'>) {
  const { isMaximized } = useWindowMaximized()

  return (
    <div
      className={cn(
        'absolute left-22 hidden items-center md:flex',
        isMaximized ? 'left-2' : 'left-22',
        className,
      )}
      data-slot="app-header-macos-left"
      data-tauri-drag-region
      {...props}
    />
  )
}
