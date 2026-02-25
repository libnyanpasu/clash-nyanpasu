import { motion } from 'framer-motion'
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
