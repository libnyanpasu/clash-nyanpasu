import { AnimatePresence, motion } from 'framer-motion'
import { ComponentProps, createContext, useContext, useId } from 'react'
import { cn } from '@nyanpasu/ui'
import * as DialogPrimitive from '@radix-ui/react-dialog'
import { Slot, Slottable } from '@radix-ui/react-slot'
import { useControllableState } from '@radix-ui/react-use-controllable-state'
import { Button, type ButtonProps } from './button'

export const ModalPortal = DialogPrimitive.Portal

export const ModalTitle = DialogPrimitive.Title

export const ModalDescription = DialogPrimitive.Description

const ModalContext = createContext<{
  open?: boolean
  layoutId?: string
}>({})

const useModalContext = () => {
  const context = useContext(ModalContext)

  if (context === undefined) {
    throw new Error(
      'Modal compound components cannot be rendered outside the Modal component',
    )
  }

  return context
}

export function ModalTrigger({
  className,
  children,
  asChild,
  ...props
}: ComponentProps<typeof DialogPrimitive.Trigger>) {
  const { layoutId } = useModalContext()

  const Comp = asChild ? Slot : 'button'

  return (
    <DialogPrimitive.Trigger
      {...props}
      asChild
      data-slot="modal-trigger"
      data-layout-id={layoutId}
    >
      <Comp className={cn('relative', className)}>
        <Slottable>{children}</Slottable>

        <motion.div
          className="absolute inset-0 -z-10 size-full"
          data-slot="modal-trigger-placeholder"
          layout
          layoutId={layoutId}
        />
      </Comp>
    </DialogPrimitive.Trigger>
  )
}

export function ModalClose({
  children,
  ...props
}: ComponentProps<typeof DialogPrimitive.Close> &
  (ComponentProps<typeof DialogPrimitive.Close>['asChild'] extends true
    ? object
    : ButtonProps)) {
  return (
    <DialogPrimitive.Close {...props} asChild>
      {props.asChild ? children : <Button>{children}</Button>}
    </DialogPrimitive.Close>
  )
}

export function ModalOverlay({
  className,
  ...props
}: ComponentProps<typeof DialogPrimitive.Overlay>) {
  return (
    <DialogPrimitive.Overlay {...props} asChild>
      <motion.div
        className={cn(
          'fixed inset-0 z-50',
          'backdrop-blur-lg',
          'bg-on-primary-container/10 dark:bg-on-primary/5',
          className,
        )}
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
      />
    </DialogPrimitive.Overlay>
  )
}

export function ModalContent({
  className,
  children,
  ...props
}: ComponentProps<typeof DialogPrimitive.Content>) {
  const { open, layoutId } = useModalContext()

  return (
    <AnimatePresence initial={false}>
      {open && (
        <ModalPortal forceMount>
          <ModalOverlay />

          <div
            className={cn(
              'fixed inset-0 z-50 grid place-items-center',
              className,
            )}
          >
            <DialogPrimitive.Content
              {...props}
              aria-describedby={undefined}
              data-slot="modal-content"
              data-layout-id={layoutId}
              asChild
            >
              <motion.div
                layout
                layoutId={layoutId}
                initial={{
                  opacity: 0,
                  scale: 0.95,
                }}
                animate={{
                  opacity: 1,
                  scale: 1,
                }}
                exit={{
                  opacity: 0,
                  scale: 0.95,
                }}
              >
                {children}
              </motion.div>
            </DialogPrimitive.Content>
          </div>
        </ModalPortal>
      )}
    </AnimatePresence>
  )
}

export function Modal({
  open: inputOpen,
  defaultOpen,
  onOpenChange,
  ...props
}: ComponentProps<typeof DialogPrimitive.Root>) {
  const layoutId = useId()

  const [open, setOpen] = useControllableState({
    prop: inputOpen,
    defaultProp: defaultOpen ?? false,
    onChange: onOpenChange,
  })

  return (
    <ModalContext.Provider
      value={{
        open,
        layoutId,
      }}
    >
      <DialogPrimitive.Root open={open} onOpenChange={setOpen} {...props} />
    </ModalContext.Provider>
  )
}
