import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { Dialog as DialogPrimitive } from 'radix-ui'
import { ComponentProps, createContext, useContext } from 'react'
import { cn } from '@nyanpasu/utils'
import { useControllableState } from '@radix-ui/react-use-controllable-state'

const DrawerOpenContext = createContext(false)

export function Drawer({
  open: controlledOpen,
  defaultOpen,
  onOpenChange,
  ...props
}: ComponentProps<typeof DialogPrimitive.Root>) {
  const [open, setOpen] = useControllableState({
    prop: controlledOpen,
    defaultProp: defaultOpen ?? false,
    onChange: onOpenChange,
  })

  return (
    <DrawerOpenContext.Provider value={open}>
      <DialogPrimitive.Root {...props} open={open} onOpenChange={setOpen} />
    </DrawerOpenContext.Provider>
  )
}

export const DrawerTitle = DialogPrimitive.Title

export const DrawerClose = DialogPrimitive.Close

export function DrawerHandle({ className, ...props }: ComponentProps<'span'>) {
  return (
    <span
      {...props}
      aria-hidden
      data-slot="drawer-handle"
      className={cn(
        'bg-outline-variant mx-auto mt-2 block h-1 w-8 shrink-0 rounded-full',
        className,
      )}
    />
  )
}

function DrawerSurface({
  className,
  children,
  ...props
}: ComponentProps<typeof motion.div>) {
  const open = useContext(DrawerOpenContext)
  const reducedMotion = useReducedMotion()

  return (
    <motion.div
      {...props}
      data-slot="drawer-content"
      inert={!open}
      aria-hidden={!open || undefined}
      className={cn(
        'fixed inset-x-0 bottom-0 z-50 mx-auto flex w-full flex-col rounded-t-3xl',
        'bg-surface text-on-surface',
        className,
      )}
      initial={reducedMotion ? { opacity: 0 } : { y: '100%' }}
      animate={{ y: 0, opacity: 1 }}
      exit={reducedMotion ? { opacity: 0 } : { y: '100%' }}
      transition={
        reducedMotion
          ? { duration: 0 }
          : { type: 'spring', stiffness: 380, damping: 40 }
      }
    >
      {children}
    </motion.div>
  )
}

export function DrawerContent({
  className,
  children,
  ...props
}: Omit<
  ComponentProps<typeof DialogPrimitive.Content>,
  'asChild' | 'forceMount'
>) {
  const open = useContext(DrawerOpenContext)
  const reducedMotion = useReducedMotion()

  return (
    <AnimatePresence>
      {open && (
        <DialogPrimitive.Portal forceMount>
          <DialogPrimitive.Overlay forceMount asChild>
            <motion.div
              data-slot="drawer-overlay"
              className="fixed inset-0 z-50 bg-black/30"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: reducedMotion ? 0 : 0.18 }}
            />
          </DialogPrimitive.Overlay>

          <DialogPrimitive.Content {...props} forceMount asChild>
            <DrawerSurface className={className}>{children}</DrawerSurface>
          </DialogPrimitive.Content>
        </DialogPrimitive.Portal>
      )}
    </AnimatePresence>
  )
}
