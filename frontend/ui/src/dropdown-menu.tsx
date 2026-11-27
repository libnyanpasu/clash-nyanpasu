import ArrowRight from '~icons/material-symbols/arrow-right-rounded'
import Check from '~icons/material-symbols/check-rounded'
import ExpandMoreRounded from '~icons/material-symbols/expand-more-rounded'
import RadioChecked from '~icons/material-symbols/radio-button-checked'
import Radio from '~icons/material-symbols/radio-button-unchecked'
import { cva } from 'class-variance-authority'
import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { DropdownMenu as DropdownMenuPrimitive, Slot } from 'radix-ui'
import { ComponentProps, createContext, useContext } from 'react'
import { cn } from '@nyanpasu/utils'
import { useControllableState } from '@radix-ui/react-use-controllable-state'
import {
  menuContentVariants,
  menuGroupVariants,
  menuItemVariants,
  menuLabelVariants,
  menuSeparatorVariants,
  MenuSurface,
  type MenuVariant,
} from './menu-surface'

export type DropdownMenuVariant = MenuVariant

type WidthType = 'auto' | 'full'

type AlignType = 'start' | 'center' | 'end'

const dropdownMenuPillTriggerVariants = cva([
  'group/pill-trigger relative inline-flex h-8 min-w-0 max-w-full cursor-pointer items-center gap-1.5',
  'rounded-full px-2.5 text-sm font-semibold select-none outline-none',
  'bg-transparent transition-colors duration-150 motion-reduce:transition-none',
  'hover:bg-current/10 data-[state=open]:bg-current/14',
  'focus-visible:outline-2 focus-visible:outline-current/60 focus-visible:-outline-offset-2',
  'disabled:pointer-events-none disabled:opacity-38',
])

const DropdownMenuContext = createContext<{
  open: boolean
  width: WidthType
  align: AlignType
  variant: DropdownMenuVariant
} | null>(null)

const DropdownMenuVariantContext = createContext<DropdownMenuVariant | null>(
  null,
)

const DropdownMenuGroupContext = createContext(false)

const useDropdownMenuContext = () => {
  const context = useContext(DropdownMenuContext)

  if (context === null) {
    throw new Error(
      'DropdownMenu compound components cannot be rendered outside the DropdownMenu component',
    )
  }

  return context
}

const useDropdownMenuVariant = () => {
  const contentVariant = useContext(DropdownMenuVariantContext)
  const rootContext = useContext(DropdownMenuContext)

  return contentVariant ?? rootContext?.variant ?? 'default'
}

export const DropdownMenu = ({
  width = 'auto',
  align = 'center',
  variant = 'default',
  open: inputOpen,
  defaultOpen,
  onOpenChange,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Root> & {
  width?: WidthType
  align?: AlignType
  variant?: DropdownMenuVariant
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
        variant,
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
      data-slot="dropdown-menu-trigger"
      className={cn('data-[state=open]:bg-on-surface/8', className)}
      {...props}
    />
  )
}

/**
 * Compact pill-shaped trigger for app bars and titlebars (Android 17 style):
 * transparent at rest, pill-tinted on hover/open, with a spring-rotated chevron.
 */
export function DropdownMenuPillTrigger({
  children,
  className,
  hideChevron = false,
  ...props
}: Omit<ComponentProps<typeof DropdownMenuPrimitive.Trigger>, 'asChild'> & {
  hideChevron?: boolean
}) {
  const { open } = useDropdownMenuContext()
  const reduceMotion = useReducedMotion()

  return (
    <DropdownMenuPrimitive.Trigger
      data-slot="dropdown-menu-pill-trigger"
      className={cn(dropdownMenuPillTriggerVariants(), className)}
      {...props}
    >
      {children}

      {!hideChevron && (
        <motion.span
          aria-hidden="true"
          data-slot="dropdown-menu-pill-trigger-chevron"
          className="inline-flex size-4.5 shrink-0 items-center justify-center opacity-80"
          animate={{ rotate: open ? 180 : 0 }}
          transition={
            reduceMotion
              ? { duration: 0 }
              : { duration: 0.16, ease: [0.2, 0, 0, 1] }
          }
        >
          <ExpandMoreRounded className="size-4.5" />
        </motion.span>
      )}
    </DropdownMenuPrimitive.Trigger>
  )
}

export function DropdownMenuGroup({
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Group>) {
  const variant = useDropdownMenuVariant()

  return (
    <DropdownMenuGroupContext.Provider value>
      <DropdownMenuPrimitive.Group
        data-slot="dropdown-menu-group"
        className={cn(menuGroupVariants({ variant }), className)}
        {...props}
      />
    </DropdownMenuGroupContext.Provider>
  )
}

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
  const variant = useDropdownMenuVariant()

  return (
    <DropdownMenuPrimitive.SubTrigger
      data-slot="dropdown-menu-sub-trigger"
      className={cn(
        menuItemVariants({ variant, openHighlight: true }),
        'pe-2',
        className,
      )}
      {...props}
    >
      <Slot.Slottable>{children}</Slot.Slottable>

      <ArrowRight
        data-slot="dropdown-menu-sub-arrow"
        aria-hidden="true"
        className="text-on-surface-variant ms-auto size-6 shrink-0 rtl:rotate-180"
      />
    </DropdownMenuPrimitive.SubTrigger>
  )
}

export function DropdownMenuSubContent({
  children,
  className,
  variant: variantProp,
  sideOffset = 4,
  collisionPadding = 8,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.SubContent> & {
  variant?: DropdownMenuVariant
}) {
  const { open } = useDropdownMenuSubContext()
  const inheritedVariant = useDropdownMenuVariant()
  const variant = variantProp ?? inheritedVariant

  return (
    <AnimatePresence initial={false}>
      {open && (
        <DropdownMenuPortal forceMount>
          <DropdownMenuVariantContext.Provider value={variant}>
            <DropdownMenuPrimitive.SubContent
              sideOffset={sideOffset}
              collisionPadding={collisionPadding}
              {...props}
              asChild
            >
              <MenuSurface
                data-slot="dropdown-menu-sub-content"
                className={cn(menuContentVariants({ variant }), className)}
              >
                {children}
              </MenuSurface>
            </DropdownMenuPrimitive.SubContent>
          </DropdownMenuVariantContext.Provider>
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
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.RadioGroup>) => {
  const variant = useDropdownMenuVariant()
  const inGroup = useContext(DropdownMenuGroupContext)

  const [value, setValue] = useControllableState({
    prop: inputValue,
    defaultProp: defaultValue != null ? String(defaultValue) : '',
    onChange: onValueChange,
  })

  return (
    <DropdownMenuRadioGroupContext.Provider value={{ value }}>
      <DropdownMenuPrimitive.RadioGroup
        data-slot="dropdown-menu-radio-group"
        className={cn(
          !inGroup && menuGroupVariants({ variant }),
          inGroup && variant === 'launcher' && 'flex flex-col gap-0.5',
          className,
        )}
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
  variant: variantProp,
  sideOffset = 4,
  collisionPadding = 8,
  style,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Content> & {
  variant?: DropdownMenuVariant
}) => {
  const {
    open,
    width,
    align,
    variant: contextVariant,
  } = useDropdownMenuContext()
  const variant = variantProp ?? contextVariant

  return (
    <AnimatePresence initial={false}>
      {open && (
        <DropdownMenuPrimitive.Portal forceMount>
          <DropdownMenuVariantContext.Provider value={variant}>
            <DropdownMenuPrimitive.Content
              align={alignProp ?? align}
              sideOffset={sideOffset}
              collisionPadding={collisionPadding}
              {...props}
              asChild
            >
              <MenuSurface
                data-slot="dropdown-menu-content"
                className={cn(menuContentVariants({ variant }), className)}
                style={{
                  ...style,
                  width:
                    width === 'full'
                      ? 'var(--radix-popper-anchor-width)'
                      : undefined,
                }}
              >
                {children}
              </MenuSurface>
            </DropdownMenuPrimitive.Content>
          </DropdownMenuVariantContext.Provider>
        </DropdownMenuPrimitive.Portal>
      )}
    </AnimatePresence>
  )
}

export const DropdownMenuItem = ({
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Item>) => {
  const variant = useDropdownMenuVariant()

  return (
    <DropdownMenuPrimitive.Item
      data-slot="dropdown-menu-item"
      className={cn(menuItemVariants({ variant }), className)}
      {...props}
    />
  )
}

export const DropdownMenuCheckboxItem = ({
  children,
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.CheckboxItem>) => {
  const variant = useDropdownMenuVariant()

  return (
    <DropdownMenuPrimitive.CheckboxItem
      data-slot="dropdown-menu-checkbox-item"
      className={cn(
        menuItemVariants({ variant, checkedHighlight: true }),
        'ps-12',
        className,
      )}
      {...props}
    >
      <DropdownMenuPrimitive.ItemIndicator
        data-slot="dropdown-menu-item-indicator"
        className="text-on-primary-container absolute start-4 top-1/2 inline-flex -translate-y-1/2"
      >
        <Check aria-hidden="true" />
      </DropdownMenuPrimitive.ItemIndicator>

      <Slot.Slottable>{children}</Slot.Slottable>
    </DropdownMenuPrimitive.CheckboxItem>
  )
}

export const DropdownMenuRadioItem = ({
  value,
  children,
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.RadioItem>) => {
  const variant = useDropdownMenuVariant()
  const context = useDropdownMenuRadioGroupContext()

  const selected = context.value === value

  return (
    <DropdownMenuPrimitive.RadioItem
      data-slot="dropdown-menu-radio-item"
      className={cn(
        menuItemVariants({ variant, checkedHighlight: true }),
        className,
      )}
      value={value}
      {...props}
    >
      <DropdownMenuPrimitive.ItemIndicator data-slot="dropdown-menu-item-indicator">
        <RadioChecked
          className="text-on-primary-container"
          aria-hidden="true"
        />
      </DropdownMenuPrimitive.ItemIndicator>

      {!selected && (
        <span aria-hidden="true">
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
  const variant = useDropdownMenuVariant()
  const inGroup = useContext(DropdownMenuGroupContext)

  return (
    <DropdownMenuPrimitive.Label
      data-slot="dropdown-menu-label"
      className={cn(menuLabelVariants({ variant, inGroup }), className)}
      {...props}
    />
  )
}

export const DropdownMenuSeparator = ({
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Separator>) => {
  const variant = useDropdownMenuVariant()

  return (
    <DropdownMenuPrimitive.Separator
      data-slot="dropdown-menu-separator"
      className={cn(menuSeparatorVariants({ variant }), className)}
      {...props}
    />
  )
}
