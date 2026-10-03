import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { Popover as PopoverPrimitive } from 'radix-ui'
import { ComponentProps, createContext, useContext } from 'react'
import { cn } from '@nyanpasu/utils'
import { useControllableState } from '@radix-ui/react-use-controllable-state'

const PopoverOpenContext = createContext(false)

export function Popover({
  open: controlledOpen,
  defaultOpen,
  onOpenChange,
  ...props
}: ComponentProps<typeof PopoverPrimitive.Root>) {
  const [open, setOpen] = useControllableState({
    prop: controlledOpen,
    defaultProp: defaultOpen ?? false,
    onChange: onOpenChange,
  })

  return (
    <PopoverOpenContext.Provider value={open}>
      <PopoverPrimitive.Root {...props} open={open} onOpenChange={setOpen} />
    </PopoverOpenContext.Provider>
  )
}

export function PopoverTrigger(
  props: ComponentProps<typeof PopoverPrimitive.Trigger>,
) {
  return <PopoverPrimitive.Trigger data-slot="popover-trigger" {...props} />
}

export function PopoverAnchor(
  props: ComponentProps<typeof PopoverPrimitive.Anchor>,
) {
  return <PopoverPrimitive.Anchor data-slot="popover-anchor" {...props} />
}

function PopoverSurface({
  children,
  className,
  style,
  'data-side': side,
  ...props
}: ComponentProps<typeof motion.div> & { 'data-side'?: string }) {
  const open = useContext(PopoverOpenContext)
  const reducedMotion = useReducedMotion()
  const collapsed =
    side === 'top'
      ? 'inset(92% 44% 0% 44% round 12px)'
      : 'inset(0% 44% 92% 44% round 12px)'

  return (
    <motion.div
      {...props}
      data-side={side}
      data-slot="popover-content"
      inert={!open}
      aria-hidden={!open || undefined}
      className={cn(
        'border-outline-variant/50 bg-surface text-on-surface z-50 flex w-72 flex-col overflow-hidden rounded-3xl border shadow-lg',
        !open && 'pointer-events-none',
        className,
      )}
      style={{
        ...style,
        maxWidth: 'calc(100vw - 24px)',
        maxHeight: 'var(--radix-popover-content-available-height)',
        transformOrigin: 'var(--radix-popover-content-transform-origin)',
      }}
      initial={
        reducedMotion
          ? { opacity: 0 }
          : { opacity: 0, clipPath: collapsed, scale: 0.96 }
      }
      animate={{
        opacity: 1,
        clipPath: 'inset(0% 0% 0% 0% round 24px)',
        scale: 1,
      }}
      exit={
        reducedMotion
          ? { opacity: 0 }
          : { opacity: 0, clipPath: collapsed, scale: 0.96 }
      }
      transition={{
        duration: reducedMotion ? 0 : open ? 0.26 : 0.18,
        ease: [0.22, 1, 0.36, 1],
      }}
    >
      <motion.div
        className="flex min-h-0 flex-auto flex-col"
        data-slot="popover-body"
        initial={{ opacity: reducedMotion ? 1 : 0 }}
        animate={{ opacity: open ? 1 : 0 }}
        transition={{
          duration: reducedMotion ? 0 : 0.12,
          delay: open && !reducedMotion ? 0.08 : 0,
        }}
      >
        {children}
      </motion.div>
    </motion.div>
  )
}

export function PopoverContent({
  children,
  className,
  style,
  side = 'bottom',
  align = 'center',
  sideOffset = 16,
  collisionPadding = 12,
  ...props
}: Omit<
  ComponentProps<typeof PopoverPrimitive.Content>,
  'asChild' | 'forceMount'
>) {
  const open = useContext(PopoverOpenContext)

  return (
    <AnimatePresence>
      {open && (
        <PopoverPrimitive.Portal forceMount>
          <PopoverPrimitive.Content
            {...props}
            forceMount
            asChild
            side={side}
            align={align}
            sideOffset={sideOffset}
            collisionPadding={collisionPadding}
          >
            <PopoverSurface className={className} style={style}>
              {children}
            </PopoverSurface>
          </PopoverPrimitive.Content>
        </PopoverPrimitive.Portal>
      )}
    </AnimatePresence>
  )
}
