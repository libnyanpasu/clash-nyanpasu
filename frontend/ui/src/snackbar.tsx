import { AnimatePresence, motion } from 'motion/react'
import { Toast as ToastPrimitive } from 'radix-ui'
import { ComponentProps } from 'react'
import { cn } from '@nyanpasu/utils'
import { useControllableState } from '@radix-ui/react-use-controllable-state'

export type SnackbarProviderProps = ComponentProps<
  typeof ToastPrimitive.Provider
>

/** Shows the snackbars rendered below it at the bottom of the window. */
export function SnackbarProvider({
  children,
  ...props
}: SnackbarProviderProps) {
  return (
    <ToastPrimitive.Provider {...props}>
      {children}

      <ToastPrimitive.Viewport
        data-slot="snackbar-viewport"
        className={cn(
          'pointer-events-none fixed inset-x-0 bottom-0 z-50',
          'flex flex-col items-center gap-2 p-4 outline-none',
        )}
      />
    </ToastPrimitive.Provider>
  )
}

export type SnackbarProps = Omit<
  ComponentProps<typeof ToastPrimitive.Root>,
  'asChild' | 'forceMount'
>

export function Snackbar({
  open: controlledOpen,
  defaultOpen,
  onOpenChange,
  className,
  children,
  ...props
}: SnackbarProps) {
  const [open, setOpen] = useControllableState({
    prop: controlledOpen,
    defaultProp: defaultOpen ?? false,
    onChange: onOpenChange,
  })

  return (
    <AnimatePresence>
      {open && (
        <ToastPrimitive.Root
          {...props}
          open
          onOpenChange={setOpen}
          forceMount
          asChild
        >
          <motion.li
            data-slot="snackbar"
            className={cn(
              'pointer-events-auto flex min-h-12 w-full items-center gap-3 sm:w-auto sm:min-w-86',
              'max-w-2xl rounded px-4 py-3.5 shadow-lg',
              'bg-inverse-surface text-inverse-on-surface text-sm',
              className,
            )}
            initial={{ opacity: 0, y: 16 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 16 }}
            transition={{ duration: 0.2, ease: 'easeOut' }}
          >
            {children}
          </motion.li>
        </ToastPrimitive.Root>
      )}
    </AnimatePresence>
  )
}
