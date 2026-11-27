import { cva } from 'class-variance-authority'
import { motion, useReducedMotion } from 'motion/react'
import { Slot } from 'radix-ui'
import type { ComponentProps } from 'react'
import { cn } from '@nyanpasu/utils'

const MotionSlot = motion.create(Slot.Root)

export type MenuVariant = 'default' | 'launcher'

const menuSurfaceVariants = cva([
  'relative z-50 w-full overflow-auto rounded-extra-small',
  'bg-surface-container-high text-on-surface',
  'shadow-sm shadow-shadow/15',
  'border border-outline-variant/50',
])

export const menuContentVariants = cva('', {
  variants: {
    variant: {
      default: '',
      launcher: [
        'flex min-w-[min(11rem,var(--radix-popper-available-width,11rem))]',
        'max-w-[var(--radix-popper-available-width,calc(100vw-1rem))]',
        'flex-col gap-1 overflow-x-hidden overflow-y-auto p-0.5',
        'rounded-none border-0 bg-transparent shadow-none',
      ],
    },
  },
  defaultVariants: {
    variant: 'default',
  },
})

export const menuGroupVariants = cva('', {
  variants: {
    variant: {
      default: '',
      launcher: [
        'flex flex-col gap-0.5',
        '[&>*:not([role=separator]):not([role=group]):first-child:not(:only-child)]:rounded-b-small',
        '[&>*:not([role=separator]):not([role=group]):last-child:not(:only-child)]:rounded-t-small',
        '[&>*:not([role=separator]):not([role=group]):not(:first-child):not(:last-child)]:rounded-small',
      ],
    },
  },
  defaultVariants: {
    variant: 'default',
  },
})

export const menuItemVariants = cva(
  [
    'relative flex cursor-pointer items-center justify-start outline-hidden select-none',
    'data-[disabled]:pointer-events-none data-[disabled]:opacity-38',
  ],
  {
    variants: {
      variant: {
        default: [
          'h-12 gap-2 p-4',
          'hover:bg-on-surface/8 data-[highlighted]:bg-on-surface/8',
        ],
        launcher: [
          'min-h-12 gap-3 px-4 py-3 text-sm',
          'rounded-extra-large',
          'bg-surface-container-high text-on-surface border border-outline-variant/50',
          'shadow-sm shadow-shadow/15',
          'overflow-hidden',
          'after:pointer-events-none after:absolute after:inset-0 after:bg-current after:opacity-0',
          'after:transition-opacity after:duration-150 motion-reduce:after:transition-none',
          'hover:after:opacity-8 data-[highlighted]:after:opacity-8',
        ],
      },
      openHighlight: {
        true: '',
        false: '',
      },
      checkedHighlight: {
        true: '',
        false: '',
      },
    },
    compoundVariants: [
      {
        variant: 'default',
        openHighlight: true,
        className: 'data-[state=open]:bg-on-surface/8',
      },
      {
        variant: 'launcher',
        openHighlight: true,
        className: 'data-[state=open]:after:opacity-8',
      },
      {
        checkedHighlight: true,
        className: [
          'data-[state=checked]:bg-primary-container data-[state=checked]:text-on-primary-container',
          'data-[state=indeterminate]:bg-primary-container data-[state=indeterminate]:text-on-primary-container',
        ],
      },
    ],
    defaultVariants: {
      variant: 'default',
      openHighlight: false,
      checkedHighlight: false,
    },
  },
)

export const menuLabelVariants = cva(
  'flex cursor-default items-center justify-start gap-2 outline-hidden',
  {
    variants: {
      variant: {
        default: 'h-12 p-4',
        launcher: 'text-primary h-auto px-4 pt-2 pb-1 text-xs font-medium',
      },
      inGroup: {
        true: '',
        false: '',
      },
    },
    compoundVariants: [
      {
        variant: 'launcher',
        inGroup: true,
        className: [
          'text-on-surface-variant min-h-9 px-4 py-2 text-xs font-medium',
          'rounded-extra-large bg-surface-container-high border-outline-variant/50 shadow-shadow/15 border shadow-sm',
        ],
      },
    ],
    defaultVariants: {
      variant: 'default',
      inGroup: false,
    },
  },
)

export const menuSeparatorVariants = cva('', {
  variants: {
    variant: {
      default: 'bg-outline-variant/50 h-px',
      launcher: 'h-1 bg-transparent',
    },
  },
  defaultVariants: {
    variant: 'default',
  },
})

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
