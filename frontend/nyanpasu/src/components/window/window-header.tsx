import { ComponentProps } from 'react'
import { cn } from '@nyanpasu/ui'

export default function WindowHeader({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn(
        'dark:bg-primary-container bg-inverse-primary flex h-10 w-full',
        className,
      )}
      data-slot="app-header"
      data-tauri-drag-region
      {...props}
    />
  )
}
