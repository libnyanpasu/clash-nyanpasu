import { ComponentProps } from 'react'
import AnimatedLogo from '@/components/logo/animated-logo'
import { cn } from '@nyanpasu/ui'

export default function WindowTitle({
  children,
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn('flex items-center gap-2', className)}
      data-slot="app-header-logo-container"
      data-tauri-drag-region
      {...props}
    >
      <AnimatedLogo className="size-5" />

      {children}
    </div>
  )
}
