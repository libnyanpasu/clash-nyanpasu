import { AnimatePresence, motion } from 'motion/react'
import { Tooltip as TooltipPrimitive } from 'radix-ui'
import { createContext, useContext, useState, type ComponentProps } from 'react'
import { cn } from '@nyanpasu/utils'

const TooltipOpenContext = createContext(false)

export type TooltipProviderProps = Omit<
  ComponentProps<typeof TooltipPrimitive.Provider>,
  'delayDuration' | 'skipDelayDuration'
> & {
  openDelay?: number
  closeDelay?: number
}

export function TooltipProvider({
  openDelay = 120,
  closeDelay = 300,
  ...props
}: TooltipProviderProps) {
  return (
    <TooltipPrimitive.Provider
      delayDuration={openDelay}
      skipDelayDuration={closeDelay}
      {...props}
    />
  )
}

export type TooltipProps = ComponentProps<typeof TooltipPrimitive.Root>

export function Tooltip({
  open: controlledOpen,
  defaultOpen = false,
  onOpenChange,
  ...props
}: TooltipProps) {
  const [internalOpen, setInternalOpen] = useState(defaultOpen)
  const open = controlledOpen ?? internalOpen

  return (
    <TooltipOpenContext.Provider value={open}>
      <TooltipPrimitive.Root
        {...props}
        open={open}
        onOpenChange={(nextOpen) => {
          if (controlledOpen === undefined) setInternalOpen(nextOpen)
          onOpenChange?.(nextOpen)
        }}
      />
    </TooltipOpenContext.Provider>
  )
}

export type TooltipTriggerProps = ComponentProps<
  typeof TooltipPrimitive.Trigger
>

export function TooltipTrigger(props: TooltipTriggerProps) {
  return <TooltipPrimitive.Trigger {...props} />
}

export type TooltipContentProps = Omit<
  ComponentProps<typeof TooltipPrimitive.Content>,
  'asChild' | 'forceMount'
>

export function TooltipContent({
  className,
  children,
  side = 'top',
  sideOffset = 8,
  ...props
}: TooltipContentProps) {
  const open = useContext(TooltipOpenContext)
  const offset = {
    top: { y: 4 },
    right: { x: -4 },
    bottom: { y: -4 },
    left: { x: 4 },
  }[side]

  return (
    <TooltipPrimitive.Portal forceMount>
      <AnimatePresence>
        {open ? (
          <TooltipPrimitive.Content
            key="tooltip-content"
            forceMount
            side={side}
            sideOffset={sideOffset}
            className="z-50"
            {...props}
          >
            <motion.div
              className={cn(
                'w-fit rounded-full border text-xs text-balance',
                'dark:text-on-surface',
                'backdrop-blur',
                'bg-primary-container/10 dark:bg-primary/10',
                'dark:border-surface-variant/30 border-inverse-surface/10',
                'overflow-hidden px-3 py-1.5',
                className,
              )}
              initial={{ opacity: 0, scale: 0.96, ...offset }}
              animate={{ opacity: 1, scale: 1, x: 0, y: 0 }}
              exit={{ opacity: 0, scale: 0.96, ...offset }}
              transition={{ duration: 0.14, ease: 'easeOut' }}
            >
              {children}
            </motion.div>
          </TooltipPrimitive.Content>
        ) : null}
      </AnimatePresence>
    </TooltipPrimitive.Portal>
  )
}
