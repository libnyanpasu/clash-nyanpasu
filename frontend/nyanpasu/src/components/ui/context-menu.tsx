import ArrowRight from '~icons/material-symbols/arrow-right-rounded'
import Check from '~icons/material-symbols/check-rounded'
import { AnimatePresence, motion } from 'framer-motion'
import { ComponentProps, createContext, useContext } from 'react'
import { cn } from '@nyanpasu/ui'
import * as ContextMenuPrimitive from '@radix-ui/react-context-menu'
import { useControllableState } from '@radix-ui/react-use-controllable-state'

const MotionContent = ({
  children,
  className,
  ...props
}: ComponentProps<typeof motion.div>) => {
  return (
    <motion.div
      className={cn(
        'relative z-50 w-full overflow-auto rounded-md',
        'dark:text-on-surface',
        'bg-inverse-on-surface/50 dark:bg-surface/50',
        'backdrop-blur-2xl',
        'dark:shadow-inverse-on-surface/50 shadow-inverse-surface/30 shadow-sm',
        'border-outline-variant/50 dark:border-outline-variant/50 border',
        className,
      )}
      style={{
        maxHeight: 'var(--radix-context-menu-content-available-height)',
      }}
      initial={{
        opacity: 0,
        scaleY: 0.9,
        transformOrigin: 'top',
      }}
      animate={{
        opacity: 1,
        scaleY: 1,
        transformOrigin: 'top',
      }}
      exit={{
        opacity: 0,
        scaleY: 0.9,
        transformOrigin: 'top',
      }}
      transition={{
        type: 'spring',
        bounce: 0,
        duration: 0.35,
      }}
      {...props}
    >
      {children}
    </motion.div>
  )
}

const ContextMenuContext = createContext<{
  open: boolean
} | null>(null)

const useContextMenuContext = () => {
  const context = useContext(ContextMenuContext)

  if (context === null) {
    throw new Error(
      'ContextMenu compound components cannot be rendered outside the ContextMenu component',
    )
  }

  return context
}

export const ContextMenu = ({
  open: inputOpen,
  onOpenChange,
  ...props
}: ComponentProps<typeof ContextMenuPrimitive.Root> & {
  open?: boolean
}) => {
  const [open, setOpen] = useControllableState({
    prop: inputOpen,
    defaultProp: false,
    onChange: onOpenChange,
  })

  return (
    <ContextMenuContext.Provider value={{ open }}>
      <ContextMenuPrimitive.Root {...props} onOpenChange={setOpen} />
    </ContextMenuContext.Provider>
  )
}

export const ContextMenuTrigger = ContextMenuPrimitive.Trigger

export const ContextMenuGroup = ContextMenuPrimitive.Group

export const ContextMenuPortal = ContextMenuPrimitive.Portal

const ContextMenuSubContext = createContext<{
  open: boolean
} | null>(null)

const useContextMenuSubContext = () => {
  const context = useContext(ContextMenuSubContext)

  if (context === null) {
    throw new Error(
      'ContextMenuSub compound components cannot be rendered outside the ContextMenuSub component',
    )
  }

  return context
}

export const ContextMenuSub = ({
  open: inputOpen,
  defaultOpen,
  onOpenChange,
  children,
  ...props
}: ComponentProps<typeof ContextMenuPrimitive.Sub>) => {
  const [open, setOpen] = useControllableState({
    prop: inputOpen,
    defaultProp: defaultOpen ?? false,
    onChange: onOpenChange,
  })

  return (
    <ContextMenuSubContext.Provider value={{ open }}>
      <ContextMenuPrimitive.Sub {...props} open={open} onOpenChange={setOpen}>
        {children}
      </ContextMenuPrimitive.Sub>
    </ContextMenuSubContext.Provider>
  )
}

export function ContextMenuSubTrigger({
  children,
  className,
  ...props
}: ComponentProps<typeof ContextMenuPrimitive.SubTrigger>) {
  return (
    <ContextMenuPrimitive.SubTrigger
      className={cn(
        'flex h-9 cursor-default items-center justify-between gap-2 px-3 outline-hidden',
        'cursor-pointer',
        'hover:bg-surface-variant',
        'dark:hover:bg-surface-variant',
        'data-[state=open]:bg-surface-variant/30',
        'dark:data-[state=open]:bg-surface-variant/30',
        className,
      )}
      {...props}
    >
      {children}

      <ArrowRight className="text-outline-variant dark:text-outline size-5" />
    </ContextMenuPrimitive.SubTrigger>
  )
}

export function ContextMenuSubContent({
  children,
  className,
  ...props
}: ComponentProps<typeof ContextMenuPrimitive.SubContent>) {
  const { open } = useContextMenuSubContext()

  return (
    <AnimatePresence initial={false}>
      {open && (
        <ContextMenuPortal forceMount>
          <ContextMenuPrimitive.SubContent {...props} asChild>
            <MotionContent className={className}>{children}</MotionContent>
          </ContextMenuPrimitive.SubContent>
        </ContextMenuPortal>
      )}
    </AnimatePresence>
  )
}

export const ContextMenuContent = ({
  children,
  className,
  ...props
}: ComponentProps<typeof ContextMenuPrimitive.Content>) => {
  const { open } = useContextMenuContext()

  return (
    <AnimatePresence initial={false}>
      {open && (
        <ContextMenuPrimitive.Portal forceMount>
          <ContextMenuPrimitive.Content {...props} asChild>
            <MotionContent
              className={cn('min-w-48', className)}
              onContextMenu={(e) => {
                e.preventDefault()
              }}
            >
              {children}
            </MotionContent>
          </ContextMenuPrimitive.Content>
        </ContextMenuPrimitive.Portal>
      )}
    </AnimatePresence>
  )
}

export const ContextMenuItem = ({
  className,
  ...props
}: ComponentProps<typeof ContextMenuPrimitive.Item>) => {
  return (
    <ContextMenuPrimitive.Item
      data-disabled={String(props.disabled)}
      className={cn(
        'flex h-9 cursor-default items-center gap-2 px-3 text-sm outline-hidden',
        'cursor-pointer',
        'data-[disabled=false]:hover:bg-surface-variant/70',
        'data-[disabled=false]:dark:hover:bg-surface-variant/50',
        'data-[disabled=true]:text-on-surface/50',
        'data-[disabled=true]:dark:text-on-surface/50',
        'data-[disabled=true]:cursor-default',
        className,
      )}
      {...props}
    />
  )
}

export const ContextMenuCheckboxItem = ({
  children,
  className,
  ...props
}: ComponentProps<typeof ContextMenuPrimitive.CheckboxItem>) => {
  return (
    <ContextMenuPrimitive.CheckboxItem
      className={cn(
        'flex h-9 cursor-default items-center justify-between gap-2 px-3 text-sm outline-hidden',
        'cursor-pointer',
        'hover:bg-surface-variant',
        'dark:hover:bg-surface-variant',
        'data-[state=checked]:bg-primary-container dark:data-[state=checked]:bg-on-primary',
        className,
      )}
      {...props}
    >
      {children}

      <ContextMenuPrimitive.ItemIndicator>
        <Check className="text-primary size-5" />
      </ContextMenuPrimitive.ItemIndicator>
    </ContextMenuPrimitive.CheckboxItem>
  )
}

export const ContextMenuRadioGroup = ContextMenuPrimitive.RadioGroup

export const ContextMenuRadioItem = ({
  children,
  className,
  ...props
}: ComponentProps<typeof ContextMenuPrimitive.RadioItem>) => {
  return (
    <ContextMenuPrimitive.RadioItem
      className={cn(
        'flex h-9 cursor-default items-center justify-between gap-2 px-3 text-sm outline-hidden',
        'cursor-pointer',
        'hover:bg-surface-variant',
        'dark:hover:bg-surface-variant',
        'data-[state=checked]:bg-primary-container dark:data-[state=checked]:bg-on-primary',
        className,
      )}
      {...props}
    >
      {children}

      <ContextMenuPrimitive.ItemIndicator>
        <Check className="text-primary size-5" />
      </ContextMenuPrimitive.ItemIndicator>
    </ContextMenuPrimitive.RadioItem>
  )
}

export const ContextMenuLabel = ({
  className,
  ...props
}: ComponentProps<typeof ContextMenuPrimitive.Label>) => {
  return (
    <ContextMenuPrimitive.Label
      className={cn(
        'text-outline-variant flex h-9 cursor-default items-center gap-2 px-3 text-xs font-medium outline-hidden',
        className,
      )}
      {...props}
    />
  )
}

export const ContextMenuSeparator = ({
  className,
  ...props
}: ComponentProps<typeof ContextMenuPrimitive.Separator>) => {
  return (
    <ContextMenuPrimitive.Separator
      className={cn('bg-outline-variant/50 h-px', className)}
      {...props}
    />
  )
}

export const ContextMenuShortcut = ({
  className,
  ...props
}: ComponentProps<'span'>) => {
  return (
    <span
      className={cn(
        'text-outline-variant ml-auto text-xs tracking-widest',
        className,
      )}
      {...props}
    />
  )
}
