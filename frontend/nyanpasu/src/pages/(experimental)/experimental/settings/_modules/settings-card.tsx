import { ComponentProps } from 'react'
import { cn } from '@nyanpasu/ui'

export function SettingsCard({ className, ...props }: ComponentProps<'div'>) {
  return <div className={cn('px-4', className)} {...props} />
}

export function SettingsCardHeader({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn('flex items-center justify-between pb-3', className)}
      {...props}
    />
  )
}

export function SettingsCardContent({
  className,
  ...props
}: ComponentProps<'div'>) {
  return <div className={cn('py-4', className)} {...props} />
}
