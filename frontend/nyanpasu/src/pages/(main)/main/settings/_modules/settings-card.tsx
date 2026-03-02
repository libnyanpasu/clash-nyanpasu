import { motion } from 'framer-motion'
import { ComponentProps } from 'react'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import { cn } from '@nyanpasu/ui'

export function SettingsLabel({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      className={cn('text-on-primary-container px-3 py-3 text-sm', className)}
      data-slot="settings-label"
      {...props}
    />
  )
}

export function SettingsGroup({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      className={cn(
        'flex flex-col gap-1 *:transition-[border-radius]',
        '[&>*:first-child:not(:only-child)]:rounded-b-sm',
        '[&>*:last-child:not(:only-child)]:rounded-t-sm',
        '[&>*:not(:first-child):not(:last-child)]:rounded-sm',
        className,
      )}
      data-slot="settings-group"
      {...props}
    />
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
      className={cn('px-5 py-6', className)}
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
      className={cn('flex items-center justify-between gap-4', className)}
      data-slot="settings-card-content-item-container"
      {...props}
    />
  )
}

export function ItemLabel({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      className={cn('flex flex-col gap-0.5', className)}
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

export function SettingsCardAnimatedItem({
  className,
  ...props
}: ComponentProps<typeof motion.div>) {
  return (
    <motion.div
      className={cn('overflow-hidden', className)}
      initial={{
        height: 0,
        opacity: 0,
      }}
      animate={{
        height: 'auto',
        opacity: 1,
      }}
      exit={{
        height: 0,
        opacity: 0,
      }}
      transition={{
        height: {
          duration: 0.2,
          ease: 'easeInOut',
        },
        opacity: {
          duration: 0.15,
        },
      }}
      {...props}
    />
  )
}
