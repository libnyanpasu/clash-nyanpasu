import { cva } from 'class-variance-authority'
import { useReducedMotion } from 'motion/react'
import { ComponentProps } from 'react'
import { cn } from '@nyanpasu/utils'

/**
 * The row of a list whose whole row is the drag source; merge it with
 * `cardVariants()`. Only non-position properties transition, because dnd-kit
 * animates the position itself. The transition is important because a group
 * that rounds its rows sets one of its own on them.
 */
export const sortableItemVariants = cva(
  [
    'relative select-none',
    'transition-[background-color,box-shadow,border-radius,scale,opacity]!',
    'motion-reduce:transition-none!',
    'focus-visible:outline-primary focus-visible:outline-2 focus-visible:-outline-offset-2',
    // The hover state layer sits over the row instead of swapping its colour.
    'before:pointer-events-none before:absolute before:inset-0 before:rounded-[inherit]',
    'before:bg-on-surface/0 before:transition-colors',
    'motion-reduce:before:transition-none',
  ],
  {
    variants: {
      draggable: {
        true: 'hover:before:bg-on-surface/8 cursor-grab touch-none active:cursor-grabbing',
        false: '',
      },
      dragging: {
        true: [
          'bg-surface-variant shadow-lg',
          'scale-[1.02] cursor-grabbing rounded-3xl!',
          'motion-reduce:scale-100',
        ],
        false: '',
      },
      disabled: {
        true: '*:opacity-38',
        false: '',
      },
    },
    defaultVariants: {
      draggable: false,
      dragging: false,
      disabled: false,
    },
  },
)

const SORTABLE_TRANSITION = {
  duration: 320,
  // Settles with a slight overshoot.
  easing: 'cubic-bezier(0.3, 1.35, 0.5, 1)',
  // Rows also glide when a menu, an addition or a removal moves them.
  idle: true,
}

/**
 * The `transition` of `useSortable` for a row of this style: how the rows
 * glide to their new places. There is none under reduced motion.
 */
export function useSortableTransition() {
  return useReducedMotion() ? null : SORTABLE_TRANSITION
}

/** The quiet position number of a sortable row; `active` tints it primary. */
export function SortableOrder({
  active,
  className,
  ...props
}: ComponentProps<'span'> & { active?: boolean }) {
  return (
    <span
      className={cn(
        'w-5 shrink-0 text-center text-sm font-medium tabular-nums',
        active ? 'text-primary' : 'text-on-surface-variant',
        className,
      )}
      {...props}
    />
  )
}
