import ArrowRight from '~icons/material-symbols/arrow-right-rounded'
import Check from '~icons/material-symbols/check-rounded'
import RadioChecked from '~icons/material-symbols/radio-button-checked'
import Radio from '~icons/material-symbols/radio-button-unchecked'
import { AnimatePresence, motion } from 'framer-motion'
import { DropdownMenu as DropdownMenuPrimitive } from 'radix-ui'
import { ComponentProps, createContext, useContext } from 'react'
import { cn } from '@nyanpasu/ui'
import { useControllableState } from '@radix-ui/react-use-controllable-state'

const MotionContent = ({
  children,
  className,
  style,
  ...props
}: ComponentProps<typeof motion.div>) => {
  return (
    <motion.div
      className={cn(
        'relative z-50 w-full overflow-auto rounded',
        'dark:text-on-surface',
        'bg-inverse-on-surface dark:bg-surface',
        'shadow-background shadow-sm',
        'border-outline-variant/50 dark:border-outline-variant/15 border',
        className,
      )}
      style={{
        ...style,
        maxHeight: 'var(--radix-popper-available-height)',
        transformOrigin:
          'var(--radix-dropdown-menu-content-transform-origin, ' +
          'var(--radix-dropdown-menu-sub-content-transform-origin, ' +
          'var(--radix-popper-transform-origin, top)))',
      }}
      initial={{
        opacity: 0,
        scaleY: 0.9,
      }}
      animate={{
        opacity: 1,
        scaleY: 1,
      }}
      exit={{
        opacity: 0,
        scaleY: 0.9,
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

type WidthType = 'auto' | 'full'

type AlignType = 'start' | 'center' | 'end'

const DropdownMenuContext = createContext<{
  open: boolean
  width: WidthType
  align: AlignType
} | null>(null)

const useDropdownMenuContext = () => {
  const context = useContext(DropdownMenuContext)

  if (context === null) {
    throw new Error(
      'DropdownMenu compound components cannot be rendered outside the DropdownMenu component',
    )
  }

  return context
}

export const DropdownMenu = ({
  width = 'auto',
  align = 'center',
  open: inputOpen,
  defaultOpen,
  onOpenChange,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Root> & {
  width?: WidthType
  align?: AlignType
}) => {
  const [open, setOpen] = useControllableState({
    prop: inputOpen,
    defaultProp: defaultOpen ?? false,
    onChange: onOpenChange,
  })

  return (
    <DropdownMenuContext.Provider
      value={{
        open,
        width,
        align,
      }}
    >
      <DropdownMenuPrimitive.Root
        {...props}
        open={open}
        onOpenChange={setOpen}
      />
    </DropdownMenuContext.Provider>
  )
}

export function DropdownMenuTrigger({
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Trigger>) {
  return (
    <DropdownMenuPrimitive.Trigger
      className={cn('data-[state=open]:bg-inverse-on-surface', className)}
      {...props}
    />
  )
}

export const DropdownMenuGroup = DropdownMenuPrimitive.Group

export const DropdownMenuPortal = DropdownMenuPrimitive.Portal

const DropdownMenuSubContext = createContext<{
  open: boolean
} | null>(null)

const useDropdownMenuSubContext = () => {
  const context = useContext(DropdownMenuSubContext)

  if (context === null) {
    throw new Error(
      'DropdownMenuSub compound components cannot be rendered outside the DropdownMenuSub component',
    )
  }

  return context
}

export const DropdownMenuSub = ({
  open: inputOpen,
  defaultOpen,
  onOpenChange,
  children,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Sub>) => {
  const [open, setOpen] = useControllableState({
    prop: inputOpen,
    defaultProp: defaultOpen ?? false,
    onChange: onOpenChange,
  })

  return (
    <DropdownMenuSubContext.Provider
      value={{
        open,
      }}
    >
      <DropdownMenuPrimitive.Sub {...props} open={open} onOpenChange={setOpen}>
        {children}
      </DropdownMenuPrimitive.Sub>
    </DropdownMenuSubContext.Provider>
  )
}

export function DropdownMenuSubTrigger({
  children,
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.SubTrigger>) {
  return (
    <DropdownMenuPrimitive.SubTrigger
      className={cn(
        'flex h-12 cursor-default items-center justify-between gap-2 p-4 pr-2 outline-hidden',
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

      <ArrowRight className="text-outline-variant dark:text-outline size-6" />
    </DropdownMenuPrimitive.SubTrigger>
  )
}

export function DropdownMenuSubContent({
  children,
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.SubContent>) {
  const { open } = useDropdownMenuSubContext()

  return (
    <AnimatePresence initial={false}>
      {open && (
        <DropdownMenuPortal forceMount>
          <DropdownMenuPrimitive.SubContent {...props} asChild>
            <MotionContent className={className}>{children}</MotionContent>
          </DropdownMenuPrimitive.SubContent>
        </DropdownMenuPortal>
      )}
    </AnimatePresence>
  )
}

const DropdownMenuRadioGroupContext = createContext<{
  value: string | null
}>({ value: null })

const useDropdownMenuRadioGroupContext = () => {
  const context = useContext(DropdownMenuRadioGroupContext)

  if (context === undefined) {
    throw new Error(
      'DropdownMenuRadioGroup compound components cannot be rendered outside the DropdownMenuRadioGroup component',
    )
  }

  return context
}

export const DropdownMenuRadioGroup = ({
  value: inputValue,
  defaultValue,
  onValueChange,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.RadioGroup>) => {
  const [value, setValue] = useControllableState({
    prop: inputValue,
    defaultProp: String(defaultValue),
    onChange: onValueChange,
  })

  return (
    <DropdownMenuRadioGroupContext.Provider value={{ value }}>
      <DropdownMenuPrimitive.RadioGroup
        {...props}
        value={value}
        onValueChange={setValue}
      />
    </DropdownMenuRadioGroupContext.Provider>
  )
}

export const DropdownMenuContent = ({
  children,
  className,
  align: alignProp,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Content>) => {
  const { open, width, align } = useDropdownMenuContext()

  return (
    <AnimatePresence initial={false}>
      {open && (
        <DropdownMenuPrimitive.Portal forceMount>
          <DropdownMenuPrimitive.Content
            align={alignProp ?? align}
            {...props}
            asChild
          >
            <MotionContent
              className={className}
              style={{
                width:
                  width === 'full'
                    ? 'var(--radix-popper-anchor-width)'
                    : undefined,
              }}
            >
              {children}
            </MotionContent>
          </DropdownMenuPrimitive.Content>
        </DropdownMenuPrimitive.Portal>
      )}
    </AnimatePresence>
  )
}

export const DropdownMenuItem = ({
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Item>) => {
  return (
    <DropdownMenuPrimitive.Item
      className={cn(
        'flex h-12 cursor-default items-center justify-between gap-2 p-4 outline-hidden',
        'cursor-pointer',
        'hover:bg-surface-variant',
        'dark:hover:bg-surface-variant',
        className,
      )}
      {...props}
    />
  )
}

export const DropdownMenuCheckboxItem = ({
  children,
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.CheckboxItem>) => {
  return (
    <DropdownMenuPrimitive.CheckboxItem
      className={cn(
        'relative flex h-12 items-center justify-between gap-2 p-4 outline-hidden',
        'cursor-pointer pl-12',
        'hover:bg-surface-variant',
        'dark:hover:bg-surface-variant',
        'data-[state=checked]:bg-primary-container dark:data-[state=checked]:bg-on-primary',
        className,
      )}
      {...props}
    >
      <DropdownMenuPrimitive.ItemIndicator className="absolute top-1/2 left-4 -translate-y-1/2">
        <Check className="text-primary" />
      </DropdownMenuPrimitive.ItemIndicator>

      {children}
    </DropdownMenuPrimitive.CheckboxItem>
  )
}

export const DropdownMenuRadioItem = ({
  value,
  children,
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.RadioItem>) => {
  const context = useDropdownMenuRadioGroupContext()

  const selected = context.value === value

  return (
    <DropdownMenuPrimitive.RadioItem
      className={cn(
        'flex h-12 cursor-default items-center justify-between gap-2 p-4 outline-hidden',
        'cursor-pointer',
        'hover:bg-surface-variant',
        'dark:hover:bg-surface-variant',
        'data-[state=checked]:bg-primary-container dark:data-[state=checked]:bg-on-primary',
        className,
      )}
      value={value}
      {...props}
    >
      <DropdownMenuPrimitive.ItemIndicator>
        <RadioChecked className="text-primary" />
      </DropdownMenuPrimitive.ItemIndicator>

      {!selected && (
        <span>
          <Radio className="text-outline-variant dark:text-outline" />
        </span>
      )}

      <div className="flex-1">{children}</div>
    </DropdownMenuPrimitive.RadioItem>
  )
}

export const DropdownMenuLabel = ({
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Label>) => {
  return (
    <DropdownMenuPrimitive.Label
      className={cn(
        'flex h-12 cursor-default items-center justify-between gap-2 p-4 outline-hidden',
        className,
      )}
      {...props}
    />
  )
}

export const DropdownMenuSeparator = ({
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Separator>) => {
  return (
    <DropdownMenuPrimitive.Separator
      className={cn('bg-outline-variant/50 h-px', className)}
      {...props}
    />
  )
}
