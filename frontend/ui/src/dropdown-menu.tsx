import ArrowRight from '~icons/material-symbols/arrow-right-rounded'
import Check from '~icons/material-symbols/check-rounded'
import RadioChecked from '~icons/material-symbols/radio-button-checked'
import Radio from '~icons/material-symbols/radio-button-unchecked'
import { AnimatePresence } from 'motion/react'
import { DropdownMenu as DropdownMenuPrimitive } from 'radix-ui'
import { ComponentProps, createContext, useContext } from 'react'
import { cn } from '@nyanpasu/utils'
import { useControllableState } from '@radix-ui/react-use-controllable-state'
import { MenuSurface } from './menu-surface'

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
      className={cn('data-[state=open]:bg-on-surface/8', className)}
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
        'hover:bg-on-surface/8 data-[highlighted]:bg-on-surface/8',
        'data-[state=open]:bg-on-surface/8',
        className,
      )}
      {...props}
    >
      {children}

      <ArrowRight className="text-on-surface-variant size-6" />
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
            <MenuSurface className={className}>{children}</MenuSurface>
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
            <MenuSurface
              className={className}
              style={{
                width:
                  width === 'full'
                    ? 'var(--radix-popper-anchor-width)'
                    : undefined,
              }}
            >
              {children}
            </MenuSurface>
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
        'flex h-12 cursor-default items-center justify-start gap-2 p-4 outline-hidden',
        'cursor-pointer',
        'hover:bg-on-surface/8 data-[highlighted]:bg-on-surface/8',
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
        'hover:bg-on-surface/8 data-[highlighted]:bg-on-surface/8',
        'data-[state=checked]:bg-primary-container data-[state=checked]:text-on-primary-container',
        className,
      )}
      {...props}
    >
      <DropdownMenuPrimitive.ItemIndicator className="absolute top-1/2 left-4 -translate-y-1/2">
        <Check className="text-on-primary-container" />
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
        'hover:bg-on-surface/8 data-[highlighted]:bg-on-surface/8',
        'data-[state=checked]:bg-primary-container data-[state=checked]:text-on-primary-container',
        className,
      )}
      value={value}
      {...props}
    >
      <DropdownMenuPrimitive.ItemIndicator>
        <RadioChecked className="text-on-primary-container" />
      </DropdownMenuPrimitive.ItemIndicator>

      {!selected && (
        <span>
          <Radio className="text-on-surface-variant" />
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
