import { AnimatePresence, motion } from 'motion/react'
import { Dialog as DialogPrimitive, Slot } from 'radix-ui'
import {
  ComponentProps,
  createContext,
  isValidElement,
  useCallback,
  useContext,
  useEffect,
  useId,
  useRef,
  useState,
  type RefObject,
} from 'react'
import { cn } from '@nyanpasu/utils'
import { useControllableState } from '@radix-ui/react-use-controllable-state'
import { Button, type ButtonProps } from './button'

export const ModalPortal = DialogPrimitive.Portal

export const ModalTitle = DialogPrimitive.Title

export const ModalDescription = DialogPrimitive.Description

const ModalContext = createContext<
  | {
      open: boolean
      setOpen: (open: boolean) => void
      layoutId: string | undefined
      setLayoutId: (id: string) => void
      registerTrigger: (id: string) => void
      originRef: RefObject<HTMLElement | null>
    }
  | undefined
>(undefined)

const useModalContext = () => {
  const context = useContext(ModalContext)

  if (context === undefined) {
    throw new Error(
      'Modal compound components cannot be rendered outside the Modal component',
    )
  }

  return context
}

/** Read the nearest Modal state, for example to close after a successful save. */
export const useModal = () => {
  const { open, setOpen } = useModalContext()

  return { open, setOpen }
}

function ModalPlaceholder({ layoutId }: { layoutId: string }) {
  const { open } = useModalContext()

  return (
    <div
      className="@container-[size] absolute inset-0 -z-10 flex items-center justify-center"
      data-slot="modal-trigger-placeholder-container"
    >
      <motion.div
        className="size-full"
        style={{
          maxWidth: 'min(100%, calc(4 * 100cqh))',
          maxHeight: 'min(100%, calc(4 * 100cqw))',
        }}
        data-slot="modal-trigger-placeholder"
        // Measure only when the shared layout changes hands; without a
        // dependency, motion measures layout on every owner re-render.
        layoutDependency={open}
        layout
        layoutId={layoutId}
      />
    </div>
  )
}

export function ModalTrigger({
  className,
  children,
  asChild,
  onClick,
  ...props
}: ComponentProps<typeof DialogPrimitive.Trigger>) {
  const { setLayoutId, registerTrigger, originRef } = useModalContext()
  const layoutId = useId()

  useEffect(() => registerTrigger(layoutId), [layoutId, registerTrigger])

  const Comp = asChild ? Slot.Root : 'button'
  const childClassName =
    asChild && isValidElement<{ className?: string }>(children)
      ? children.props.className
      : undefined

  return (
    <DialogPrimitive.Trigger
      {...props}
      asChild
      onClick={(event) => {
        onClick?.(event)
        if (!event.defaultPrevented) {
          originRef.current = event.currentTarget
          setLayoutId(layoutId)
        }
      }}
      data-slot="modal-trigger"
      data-layout-id={layoutId}
    >
      <Comp className={cn('relative', childClassName, className)}>
        <Slot.Slottable>{children}</Slot.Slottable>

        <ModalPlaceholder layoutId={layoutId} />
      </Comp>
    </DialogPrimitive.Trigger>
  )
}

/** A persistent animation origin for menu, tray, or other programmatic opens.
 * Keep it mounted while the content exits. Pointer/focus selects this origin
 * when one Modal serves several menu anchors.
 */
export function ModalAnchor({
  asChild,
  className,
  children,
  onPointerDown,
  onFocus,
  ...props
}: ComponentProps<'div'> & { asChild?: boolean }) {
  const { setLayoutId, registerTrigger, originRef } = useModalContext()
  const layoutId = useId()
  const Comp = asChild ? Slot.Root : 'div'
  const childClassName =
    asChild && isValidElement<{ className?: string }>(children)
      ? children.props.className
      : undefined

  useEffect(() => registerTrigger(layoutId), [layoutId, registerTrigger])

  return (
    <Comp
      data-slot="modal-anchor"
      {...props}
      className={cn('relative', childClassName, className)}
      onPointerDown={(event) => {
        onPointerDown?.(event)
        if (!event.defaultPrevented) {
          const target =
            event.target instanceof Element
              ? event.target.closest<HTMLElement>('button, a, [tabindex]')
              : null
          originRef.current =
            target && event.currentTarget.contains(target)
              ? target
              : event.currentTarget
          setLayoutId(layoutId)
        }
      }}
      onFocus={(event) => {
        onFocus?.(event)
        if (!event.defaultPrevented) {
          originRef.current = event.target
          setLayoutId(layoutId)
        }
      }}
    >
      <Slot.Slottable>{children}</Slot.Slottable>
      <ModalPlaceholder layoutId={layoutId} />
    </Comp>
  )
}

export function ModalClose({
  children,
  asChild,
  ...props
}: ComponentProps<typeof DialogPrimitive.Close> & ButtonProps) {
  return (
    <DialogPrimitive.Close asChild>
      {asChild ? (
        <Slot.Root {...props}>{children}</Slot.Root>
      ) : (
        <Button {...props}>{children}</Button>
      )}
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
        data-slot="modal-overlay"
        className={cn(
          'fixed inset-0 z-50',
          'backdrop-blur-xl',
          'bg-on-primary-container/30 dark:bg-on-primary/15',
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
  onExitComplete,
  dismissible = true,
  onEscapeKeyDown,
  onInteractOutside,
  onOpenAutoFocus,
  onCloseAutoFocus,
  ...props
}: ComponentProps<typeof DialogPrimitive.Content> & {
  /** Clean up selection only after the closing animation has finished. */
  onExitComplete?: () => void
  /** Block Escape and outside dismissal while an operation is pending. */
  dismissible?: boolean
}) {
  const { open, layoutId, originRef } = useModalContext()
  const returnFocusRef = useRef<HTMLElement | null>(null)

  return (
    <AnimatePresence initial={false} onExitComplete={onExitComplete}>
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
              onOpenAutoFocus={(event) => {
                const active = document.activeElement
                returnFocusRef.current =
                  active instanceof HTMLElement && active !== document.body
                    ? active
                    : null
                onOpenAutoFocus?.(event)
              }}
              onCloseAutoFocus={(event) => {
                onCloseAutoFocus?.(event)
                const target = originRef.current?.isConnected
                  ? originRef.current
                  : returnFocusRef.current
                if (!event.defaultPrevented && target?.isConnected) {
                  event.preventDefault()
                  target.focus()
                }
              }}
              onEscapeKeyDown={(event) => {
                onEscapeKeyDown?.(event)
                if (!dismissible) event.preventDefault()
              }}
              onInteractOutside={(event) => {
                onInteractOutside?.(event)
                if (!dismissible) event.preventDefault()
              }}
              data-slot="modal-content"
              data-layout-id={layoutId}
              asChild
            >
              <motion.div
                // A constant keeps the open dialog from being re-measured each
                // time its content re-renders. It must differ from the
                // placeholder's dependency: motion hands a shared layout over
                // only between members whose dependencies differ.
                layoutDependency="modal-content"
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
  const originRef = useRef<HTMLElement | null>(null)
  const [layoutId, setLayoutId] = useState<string>()
  const registerTrigger = useCallback((id: string) => {
    setLayoutId((current) => current ?? id)
  }, [])

  const [open, setOpen] = useControllableState({
    prop: inputOpen,
    defaultProp: defaultOpen ?? false,
    onChange: onOpenChange,
  })

  return (
    <ModalContext.Provider
      value={{
        open,
        setOpen,
        layoutId,
        setLayoutId,
        registerTrigger,
        originRef,
      }}
    >
      <DialogPrimitive.Root open={open} onOpenChange={setOpen} {...props} />
    </ModalContext.Provider>
  )
}
