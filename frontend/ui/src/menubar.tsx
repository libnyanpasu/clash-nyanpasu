import ArrowRight from '~icons/material-symbols/arrow-right-rounded'
import Check from '~icons/material-symbols/check-rounded'
import RadioChecked from '~icons/material-symbols/radio-button-checked'
import Radio from '~icons/material-symbols/radio-button-unchecked'
import Remove from '~icons/material-symbols/remove-rounded'
import { cva } from 'class-variance-authority'
import { Menubar as MenubarPrimitive, Slot } from 'radix-ui'
import { createContext, useContext, type ComponentProps } from 'react'
import { cn } from '@nyanpasu/utils'
import {
  menuContentVariants,
  menuGroupVariants,
  menuItemVariants,
  menuLabelVariants,
  menuSeparatorVariants,
  MenuSurface,
  type MenuVariant,
} from './menu-surface'

export const menubarTriggerVariants = cva([
  'flex min-h-10 cursor-pointer items-center gap-2 rounded-full px-3 py-2',
  'text-primary text-sm font-medium outline-none select-none',
  'hover:bg-on-surface/8',
  'data-[state=open]:bg-on-surface/8',
  'transition-colors motion-reduce:transition-none',
  'focus-visible:outline-2 focus-visible:outline-primary focus-visible:-outline-offset-2',
  'data-[disabled]:pointer-events-none data-[disabled]:opacity-38',
])

const MenubarContext = createContext<{ variant: MenuVariant }>({
  variant: 'default',
})

const MenubarVariantContext = createContext<MenuVariant | null>(null)

const MenubarGroupContext = createContext(false)

const useMenubarVariant = () => {
  const contentVariant = useContext(MenubarVariantContext)
  const rootContext = useContext(MenubarContext)

  return contentVariant ?? rootContext.variant
}

/** A horizontal menu bar. Radix owns its open menu and keyboard navigation. */
export function Menubar({
  className,
  variant = 'default',
  ...props
}: ComponentProps<typeof MenubarPrimitive.Root> & {
  variant?: MenuVariant
}) {
  return (
    <MenubarContext.Provider value={{ variant }}>
      <MenubarPrimitive.Root
        data-slot="menubar"
        className={cn('flex items-center gap-0.5', className)}
        {...props}
      />
    </MenubarContext.Provider>
  )
}

export const MenubarMenu = MenubarPrimitive.Menu
export const MenubarSub = MenubarPrimitive.Sub
export const MenubarPortal = MenubarPrimitive.Portal

export function MenubarTrigger({
  className,
  ...props
}: ComponentProps<typeof MenubarPrimitive.Trigger>) {
  return (
    <MenubarPrimitive.Trigger
      data-slot="menubar-trigger"
      className={cn(menubarTriggerVariants(), className)}
      {...props}
    />
  )
}

/** Menu surfaces are portaled by default so layout containers cannot clip them. */
export function MenubarContent({
  className,
  align = 'start',
  sideOffset = 4,
  collisionPadding = 8,
  variant: variantProp,
  style,
  children,
  asChild,
  forceMount,
  ...props
}: ComponentProps<typeof MenubarPrimitive.Content> & {
  variant?: MenuVariant
}) {
  const inheritedVariant = useMenubarVariant()
  const variant = variantProp ?? inheritedVariant

  return (
    <MenubarPortal forceMount={forceMount}>
      <MenubarVariantContext.Provider value={variant}>
        <MenubarPrimitive.Content
          data-slot="menubar-content"
          align={align}
          sideOffset={sideOffset}
          collisionPadding={collisionPadding}
          forceMount={forceMount}
          {...props}
          asChild
        >
          <MenuSurface
            asChild={asChild}
            className={cn(menuContentVariants({ variant }), className)}
            style={style}
          >
            {children}
          </MenuSurface>
        </MenubarPrimitive.Content>
      </MenubarVariantContext.Provider>
    </MenubarPortal>
  )
}

export function MenubarGroup({
  className,
  ...props
}: ComponentProps<typeof MenubarPrimitive.Group>) {
  const variant = useMenubarVariant()

  return (
    <MenubarGroupContext.Provider value>
      <MenubarPrimitive.Group
        data-slot="menubar-group"
        className={cn(menuGroupVariants({ variant }), className)}
        {...props}
      />
    </MenubarGroupContext.Provider>
  )
}

export function MenubarItem({
  className,
  ...props
}: ComponentProps<typeof MenubarPrimitive.Item>) {
  const variant = useMenubarVariant()

  return (
    <MenubarPrimitive.Item
      data-slot="menubar-item"
      className={cn(menuItemVariants({ variant }), className)}
      {...props}
    />
  )
}

export function MenubarItemIndicator({
  className,
  ...props
}: ComponentProps<typeof MenubarPrimitive.ItemIndicator>) {
  return (
    <MenubarPrimitive.ItemIndicator
      data-slot="menubar-item-indicator"
      className={cn(
        'text-on-primary-container absolute start-4 top-1/2 inline-flex -translate-y-1/2',
        className,
      )}
      {...props}
    />
  )
}

export function MenubarCheckboxItem({
  className,
  children,
  checked,
  ...props
}: ComponentProps<typeof MenubarPrimitive.CheckboxItem>) {
  const variant = useMenubarVariant()

  return (
    <MenubarPrimitive.CheckboxItem
      data-slot="menubar-checkbox-item"
      checked={checked}
      className={cn(
        menuItemVariants({ variant, checkedHighlight: true }),
        'ps-12',
        className,
      )}
      {...props}
    >
      <MenubarItemIndicator>
        {checked === 'indeterminate' ? (
          <Remove aria-hidden="true" />
        ) : (
          <Check aria-hidden="true" />
        )}
      </MenubarItemIndicator>

      <Slot.Slottable>{children}</Slot.Slottable>
    </MenubarPrimitive.CheckboxItem>
  )
}

export function MenubarRadioGroup({
  className,
  ...props
}: ComponentProps<typeof MenubarPrimitive.RadioGroup>) {
  const variant = useMenubarVariant()
  const inGroup = useContext(MenubarGroupContext)

  return (
    <MenubarPrimitive.RadioGroup
      data-slot="menubar-radio-group"
      className={cn(
        !inGroup && menuGroupVariants({ variant }),
        inGroup && variant === 'launcher' && 'flex flex-col gap-0.5',
        className,
      )}
      {...props}
    />
  )
}

export function MenubarRadioItem({
  className,
  children,
  ...props
}: ComponentProps<typeof MenubarPrimitive.RadioItem>) {
  const variant = useMenubarVariant()

  return (
    <MenubarPrimitive.RadioItem
      data-slot="menubar-radio-item"
      className={cn(
        menuItemVariants({ variant, checkedHighlight: true }),
        'group/menubar-radio-item',
        className,
      )}
      {...props}
    >
      <MenubarItemIndicator className="static translate-y-0">
        <RadioChecked aria-hidden="true" />
      </MenubarItemIndicator>

      <span className="group-data-[state=checked]/menubar-radio-item:hidden">
        <Radio className="text-on-surface-variant" aria-hidden="true" />
      </span>

      <Slot.Slottable>{children}</Slot.Slottable>
    </MenubarPrimitive.RadioItem>
  )
}

export function MenubarSubTrigger({
  className,
  children,
  ...props
}: ComponentProps<typeof MenubarPrimitive.SubTrigger>) {
  const variant = useMenubarVariant()

  return (
    <MenubarPrimitive.SubTrigger
      data-slot="menubar-sub-trigger"
      className={cn(
        menuItemVariants({ variant, openHighlight: true }),
        'pe-2',
        className,
      )}
      {...props}
    >
      <Slot.Slottable>{children}</Slot.Slottable>

      <ArrowRight
        data-slot="menubar-sub-arrow"
        aria-hidden="true"
        className="text-on-surface-variant ms-auto size-6 shrink-0 rtl:rotate-180"
      />
    </MenubarPrimitive.SubTrigger>
  )
}

export function MenubarSubContent({
  className,
  sideOffset = 4,
  collisionPadding = 8,
  variant: variantProp,
  style,
  children,
  asChild,
  forceMount,
  ...props
}: ComponentProps<typeof MenubarPrimitive.SubContent> & {
  variant?: MenuVariant
}) {
  const inheritedVariant = useMenubarVariant()
  const variant = variantProp ?? inheritedVariant

  return (
    <MenubarPortal forceMount={forceMount}>
      <MenubarVariantContext.Provider value={variant}>
        <MenubarPrimitive.SubContent
          data-slot="menubar-sub-content"
          sideOffset={sideOffset}
          collisionPadding={collisionPadding}
          forceMount={forceMount}
          {...props}
          asChild
        >
          <MenuSurface
            asChild={asChild}
            className={cn(menuContentVariants({ variant }), className)}
            style={style}
          >
            {children}
          </MenuSurface>
        </MenubarPrimitive.SubContent>
      </MenubarVariantContext.Provider>
    </MenubarPortal>
  )
}

export function MenubarLabel({
  className,
  ...props
}: ComponentProps<typeof MenubarPrimitive.Label>) {
  const variant = useMenubarVariant()
  const inGroup = useContext(MenubarGroupContext)

  return (
    <MenubarPrimitive.Label
      data-slot="menubar-label"
      className={cn(menuLabelVariants({ variant, inGroup }), className)}
      {...props}
    />
  )
}

export function MenubarSeparator({
  className,
  ...props
}: ComponentProps<typeof MenubarPrimitive.Separator>) {
  const variant = useMenubarVariant()

  return (
    <MenubarPrimitive.Separator
      data-slot="menubar-separator"
      className={cn(menuSeparatorVariants({ variant }), className)}
      {...props}
    />
  )
}

export function MenubarArrow({
  className,
  ...props
}: ComponentProps<typeof MenubarPrimitive.Arrow>) {
  return (
    <MenubarPrimitive.Arrow
      data-slot="menubar-arrow"
      className={cn('fill-surface-container-high', className)}
      {...props}
    />
  )
}
