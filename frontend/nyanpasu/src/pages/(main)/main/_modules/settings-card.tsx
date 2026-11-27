import { ComponentProps } from 'react'
import { AnimatedItem } from '@nyanpasu/ui/animated-item'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { ItemGroup, itemSectionTitleVariants } from '@nyanpasu/ui/item-group'
import { cn } from '@nyanpasu/utils'

export function SettingsLabel({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      className={cn(itemSectionTitleVariants(), 'px-4 pt-3 pb-2', className)}
      data-slot="settings-label"
      {...props}
    />
  )
}

export function SettingsGroup({
  className,
  ...props
}: ComponentProps<typeof ItemGroup>) {
  return (
    <ItemGroup className={className} data-slot="settings-group" {...props} />
  )
}

export function SettingsCard({
  className,
  ...props
}: ComponentProps<typeof Card>) {
  return <Card className={cn(className)} data-slot="settings-card" {...props} />
}

export function SettingsCardHeader({
  className,
  ...props
}: ComponentProps<typeof CardHeader>) {
  return (
    <CardHeader
      className={cn('px-5 pt-6', className)}
      data-slot="settings-card-header"
      {...props}
    />
  )
}

export function SettingsCardFooter({
  className,
  ...props
}: ComponentProps<typeof CardFooter>) {
  return (
    <CardFooter
      className={cn('px-3 pb-3', className)}
      data-slot="settings-card-footer"
      {...props}
    />
  )
}

export function SettingsCardContent({
  className,
  ...props
}: ComponentProps<typeof CardContent>) {
  return (
    <CardContent
      className={cn('gap-6 px-5 py-6', className)}
      data-slot="settings-card-content"
      {...props}
    />
  )
}

export function ItemContainer({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      className={cn(
        'flex w-full min-w-0 items-center justify-between gap-4 whitespace-normal',
        className,
      )}
      data-slot="settings-card-content-item-container"
      {...props}
    />
  )
}

export function ItemLabel({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      className={cn(
        'flex min-w-0 flex-1 flex-col gap-0.5 text-left',
        className,
      )}
      data-slot="settings-card-content-item-label"
      {...props}
    />
  )
}

export function ItemLabelText({
  className,
  ...props
}: ComponentProps<typeof CardContent>) {
  return (
    <p
      className={cn('text-base font-medium', className)}
      data-slot="settings-card-content-item-label-text"
      {...props}
    />
  )
}

export function ItemLabelDescription({
  className,
  ...props
}: ComponentProps<'p'>) {
  return (
    <p
      className={cn('text-on-surface-variant text-sm', className)}
      data-slot="settings-card-content-item-label-description"
      {...props}
    />
  )
}

export const SettingsCardAnimatedItem = AnimatedItem
