import { cva } from 'class-variance-authority'
import { motion, useReducedMotion } from 'motion/react'
import { Slot } from 'radix-ui'
import type { ComponentProps } from 'react'
import { cn } from '@nyanpasu/utils'

const MotionSlot = motion.create(Slot.Root)

const menuSurfaceVariants = cva([
  'relative z-50 w-full overflow-auto rounded',
  'dark:text-on-surface',
  'bg-inverse-on-surface dark:bg-surface',
  'shadow-background shadow-sm',
  'border-outline-variant/50 dark:border-outline-variant/15 border',
])

/** The shared popup surface and opening motion for project menus. */
export function MenuSurface({
  asChild,
  children,
  className,
  style,
  ...props
}: ComponentProps<typeof motion.div> & { asChild?: boolean }) {
  const reduceMotion = useReducedMotion()
  const Surface = asChild ? MotionSlot : motion.div

  return (
    <Surface
      className={cn(menuSurfaceVariants(), className)}
      style={{
        ...style,
        maxHeight: 'var(--radix-popper-available-height)',
        transformOrigin:
          'var(--radix-menubar-content-transform-origin, ' +
          'var(--radix-dropdown-menu-content-transform-origin, ' +
          'var(--radix-dropdown-menu-sub-content-transform-origin, ' +
          'var(--radix-popper-transform-origin, top))))',
      }}
      initial={reduceMotion ? false : { opacity: 0, scaleY: 0.9 }}
      animate={{ opacity: 1, scaleY: 1 }}
      exit={{ opacity: 0, scaleY: 0.9 }}
      transition={
        reduceMotion
          ? { duration: 0 }
          : { type: 'spring', bounce: 0, duration: 0.35 }
      }
      {...props}
    >
      {children}
    </Surface>
  )
}
