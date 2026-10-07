import CheckRounded from '~icons/material-symbols/check-rounded'
import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { Toggle as TogglePrimitive } from 'radix-ui'
import { ComponentProps, ReactNode } from 'react'
import { cn } from '@nyanpasu/utils'
import { useControllableState } from '@radix-ui/react-use-controllable-state'

export type FilterChipProps = Omit<
  ComponentProps<typeof TogglePrimitive.Root>,
  'children'
> & {
  children: ReactNode
}

export const FilterChip = ({
  pressed: controlledPressed,
  defaultPressed,
  onPressedChange,
  className,
  children,
  ...props
}: FilterChipProps) => {
  const reduceMotion = useReducedMotion()

  const [pressed, setPressed] = useControllableState({
    prop: controlledPressed,
    defaultProp: defaultPressed ?? false,
    onChange: onPressedChange,
  })

  return (
    <TogglePrimitive.Root
      data-slot="filter-chip"
      className={cn(
        'inline-flex h-8 items-center gap-2 rounded-lg px-4',
        'cursor-pointer text-sm font-medium whitespace-nowrap select-none',
        'transition-[background-color,border-color,color,padding]',
        'border-outline-variant text-on-surface-variant border',
        'hover:bg-on-surface-variant/8',
        'data-[state=on]:bg-secondary-container data-[state=on]:border-transparent',
        'data-[state=on]:text-on-secondary-container data-[state=on]:pl-2',
        'data-[state=on]:hover:bg-secondary-container data-[state=on]:hover:brightness-95',
        'dark:data-[state=on]:hover:brightness-105',
        'focus-visible:outline-primary outline-none focus-visible:outline-2',
        'disabled:cursor-not-allowed disabled:opacity-38 disabled:hover:bg-transparent',
        className,
      )}
      pressed={pressed}
      onPressedChange={setPressed}
      {...props}
    >
      <AnimatePresence initial={false}>
        {pressed && (
          <motion.span
            aria-hidden
            data-slot="filter-chip-check"
            className="inline-flex shrink-0 overflow-hidden"
            initial={{ width: 0, opacity: 0, marginRight: -8 }}
            animate={{ width: 18, opacity: 1, marginRight: 0 }}
            exit={{ width: 0, opacity: 0, marginRight: -8 }}
            transition={
              reduceMotion
                ? { duration: 0 }
                : { duration: 0.2, ease: 'easeOut' }
            }
          >
            <CheckRounded className="size-[18px] shrink-0" />
          </motion.span>
        )}
      </AnimatePresence>

      {children}
    </TogglePrimitive.Root>
  )
}
