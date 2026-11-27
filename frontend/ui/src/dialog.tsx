import { cva, type VariantProps } from 'class-variance-authority'
import { AnimatePresence } from 'motion/react'
import {
  useCallback,
  type ComponentProps,
  type FormEvent,
  type ReactNode,
} from 'react'
import { useLockFn } from '@nyanpasu/hooks'
import { cn } from '@nyanpasu/utils'
import { AnimatedItem } from './animated-item'
import { Button } from './button'
import {
  Modal,
  ModalAnchor,
  ModalClose,
  ModalContent,
  ModalDescription,
  ModalTitle,
  ModalTrigger,
  useModal,
} from './modal'

export const Dialog = Modal
export const DialogTrigger = ModalTrigger
export const DialogAnchor = ModalAnchor
export const DialogClose = ModalClose
export const useDialog = useModal

export const dialogSurfaceVariants = cva(
  [
    'flex max-h-[85vh] flex-col gap-5 overflow-hidden p-6',
    'rounded-extra-large shadow-xl',
    'bg-surface-container-high text-on-surface',
  ],
  {
    variants: {
      size: {
        sm: 'w-[min(92vw,22rem)]',
        md: 'w-[min(92vw,26rem)]',
        lg: 'w-[min(92vw,34rem)]',
        xl: 'w-[min(92vw,42rem)]',
      },
    },
    defaultVariants: {
      size: 'md',
    },
  },
)

export type DialogSurfaceVariantsProps = VariantProps<
  typeof dialogSurfaceVariants
>

export interface DialogContentProps
  extends ComponentProps<typeof ModalContent>, DialogSurfaceVariantsProps {
  /** Class name applied to the inner dialog surface container. */
  surfaceClassName?: string
}

export function DialogContent({
  size,
  className,
  surfaceClassName,
  children,
  ...props
}: DialogContentProps) {
  return (
    <ModalContent className={className} {...props}>
      <div
        data-slot="dialog-surface"
        className={cn(dialogSurfaceVariants({ size }), surfaceClassName)}
      >
        {children}
      </div>
    </ModalContent>
  )
}

export function DialogHeader({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      data-slot="dialog-header"
      className={cn('flex shrink-0 flex-col gap-1.5', className)}
      {...props}
    />
  )
}

export function DialogTitle({
  className,
  ...props
}: ComponentProps<typeof ModalTitle>) {
  return (
    <ModalTitle
      data-slot="dialog-title"
      className={cn(
        'text-on-surface text-xl leading-snug font-medium',
        className,
      )}
      {...props}
    />
  )
}

export function DialogDescription({
  className,
  ...props
}: ComponentProps<typeof ModalDescription>) {
  return (
    <ModalDescription
      data-slot="dialog-description"
      className={cn('text-on-surface-variant text-sm leading-5', className)}
      {...props}
    />
  )
}

export function DialogBody({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      data-slot="dialog-body"
      className={cn(
        'flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto',
        className,
      )}
      {...props}
    />
  )
}

export function DialogFooter({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      data-slot="dialog-footer"
      className={cn(
        'flex shrink-0 items-center justify-end gap-2 pt-1',
        className,
      )}
      {...props}
    />
  )
}

export function FormDialogError({
  message,
  id,
  className,
}: {
  message?: ReactNode
  id?: string
  className?: string
}) {
  return (
    <AnimatePresence initial={false}>
      {message != null && message !== '' && (
        <AnimatedItem
          id={id}
          role="alert"
          aria-live="polite"
          data-slot="form-dialog-error"
          className={cn('text-error text-xs leading-4', className)}
        >
          {message}
        </AnimatedItem>
      )}
    </AnimatePresence>
  )
}

export interface FormDialogContentProps extends Omit<
  DialogContentProps,
  'onSubmit' | 'title'
> {
  title: ReactNode
  description?: ReactNode
  submitLabel: ReactNode
  cancelLabel: ReactNode
  /** Locks dismissal and disables buttons while an async submission is pending. */
  submitting?: boolean
  submitDisabled?: boolean
  /** Optional extra buttons placed before the cancel/submit pair (e.g. Reset). */
  extraActions?: ReactNode
  /** Called when the dialog opens (onOpenAutoFocus), e.g. to reset draft fields. */
  onOpen?: () => void
  onSubmit: (event: FormEvent<HTMLFormElement>) => void | Promise<void>
  noValidate?: boolean
}

export function FormDialogContent({
  title,
  description,
  submitLabel,
  cancelLabel,
  submitting = false,
  submitDisabled = false,
  extraActions,
  onOpen,
  onOpenAutoFocus,
  onSubmit,
  noValidate = true,
  children,
  ...props
}: FormDialogContentProps) {
  const lockedSubmit = useLockFn(async (event: FormEvent<HTMLFormElement>) => {
    await onSubmit(event)
  })

  const handleFormSubmit = useCallback(
    (event: FormEvent<HTMLFormElement>) => {
      event.preventDefault()

      if (submitting || submitDisabled) {
        return
      }

      lockedSubmit(event)
    },
    [lockedSubmit, submitting, submitDisabled],
  )

  return (
    <DialogContent
      dismissible={!submitting}
      onOpenAutoFocus={(event) => {
        onOpen?.()
        onOpenAutoFocus?.(event)
      }}
      {...props}
    >
      <DialogHeader>
        <DialogTitle>{title}</DialogTitle>

        {description != null && (
          <DialogDescription>{description}</DialogDescription>
        )}
      </DialogHeader>

      <form
        data-slot="form-dialog-form"
        className="flex min-h-0 flex-1 flex-col gap-5"
        noValidate={noValidate}
        onSubmit={handleFormSubmit}
      >
        <DialogBody>{children}</DialogBody>

        <DialogFooter>
          {extraActions}

          <DialogClose type="button" variant="stroked" disabled={submitting}>
            {cancelLabel}
          </DialogClose>

          <Button
            type="submit"
            variant="flat"
            loading={submitting}
            disabled={submitting || submitDisabled}
          >
            {submitLabel}
          </Button>
        </DialogFooter>
      </form>
    </DialogContent>
  )
}

export interface FormDialogProps extends Omit<
  FormDialogContentProps,
  'children'
> {
  open?: boolean
  defaultOpen?: boolean
  onOpenChange?: (open: boolean) => void
  /** Optional trigger element wrapped in `DialogTrigger asChild`. */
  trigger?: ReactNode
  children: ReactNode
}

export function FormDialog({
  open,
  defaultOpen,
  onOpenChange,
  trigger,
  children,
  ...contentProps
}: FormDialogProps) {
  return (
    <Dialog open={open} defaultOpen={defaultOpen} onOpenChange={onOpenChange}>
      {trigger != null && <DialogTrigger asChild>{trigger}</DialogTrigger>}

      <FormDialogContent {...contentProps}>{children}</FormDialogContent>
    </Dialog>
  )
}
