import ArrowRight from '~icons/material-symbols/arrow-right-rounded'
import Check from '~icons/material-symbols/check-rounded'
import RadioChecked from '~icons/material-symbols/radio-button-checked'
import Radio from '~icons/material-symbols/radio-button-unchecked'
import Remove from '~icons/material-symbols/remove-rounded'
import { cva } from 'class-variance-authority'
import { Menubar as MenubarPrimitive, Slot } from 'radix-ui'
import type { ComponentProps } from 'react'
import { cn } from '@nyanpasu/utils'
import { MenuSurface } from './menu-surface'

const menubarTriggerVariants = cva([
  'flex min-h-10 cursor-pointer items-center gap-2 rounded-full px-3 py-2',
  'text-primary dark:text-primary text-sm font-medium outline-none select-none',
  'hover:bg-primary-container dark:hover:bg-on-primary',
  'data-[state=open]:bg-primary-container dark:data-[state=open]:bg-on-primary',
  'transition-colors motion-reduce:transition-none',
  'focus-visible:outline-2 focus-visible:outline-primary focus-visible:-outline-offset-2',
  'data-[disabled]:pointer-events-none data-[disabled]:opacity-38',
])

const menubarItemVariants = cva(
  [
    'relative flex h-12 cursor-pointer items-center justify-start gap-2 p-4 outline-hidden',
    'hover:bg-surface-variant dark:hover:bg-surface-variant',
    'data-[highlighted]:bg-surface-variant',
    'data-[disabled]:pointer-events-none data-[disabled]:opacity-38',
  ],
  {
    variants: {
      indicator: {
        true: [
          'ps-12',
          'data-[state=checked]:bg-primary-container dark:data-[state=checked]:bg-on-primary',
          'data-[state=indeterminate]:bg-primary-container',
          'dark:data-[state=indeterminate]:bg-on-primary',
        ],
      },
    },
  },
)

/** A horizontal menu bar. Radix owns its open menu and keyboard navigation. */
export function Menubar({
  className,
  ...props
}: ComponentProps<typeof MenubarPrimitive.Root>) {
  return (
    <MenubarPrimitive.Root
      data-slot="menubar"
      className={cn('flex items-center gap-0.5', className)}
      {...props}
    />
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
  style,
  children,
  asChild,
  forceMount,
  ...props
}: ComponentProps<typeof MenubarPrimitive.Content>) {
  return (
    <MenubarPortal forceMount={forceMount}>
      <MenubarPrimitive.Content
        data-slot="menubar-content"
        align={align}
        sideOffset={sideOffset}
        forceMount={forceMount}
        {...props}
        asChild
      >
        <MenuSurface asChild={asChild} className={className} style={style}>
          {children}
        </MenuSurface>
      </MenubarPrimitive.Content>
    </MenubarPortal>
  )
}

export function MenubarGroup(
  props: ComponentProps<typeof MenubarPrimitive.Group>,
) {
  return <MenubarPrimitive.Group data-slot="menubar-group" {...props} />
}

export function MenubarItem({
  className,
  ...props
}: ComponentProps<typeof MenubarPrimitive.Item>) {
  return (
    <MenubarPrimitive.Item
      data-slot="menubar-item"
      className={cn(menubarItemVariants(), className)}
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
        'text-primary absolute start-4 top-1/2 inline-flex -translate-y-1/2',
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
  return (
    <MenubarPrimitive.CheckboxItem
      data-slot="menubar-checkbox-item"
      checked={checked}
      className={cn(menubarItemVariants({ indicator: true }), className)}
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

export function MenubarRadioGroup(
  props: ComponentProps<typeof MenubarPrimitive.RadioGroup>,
) {
  return (
    <MenubarPrimitive.RadioGroup data-slot="menubar-radio-group" {...props} />
  )
}

export function MenubarRadioItem({
  className,
  children,
  ...props
}: ComponentProps<typeof MenubarPrimitive.RadioItem>) {
  return (
    <MenubarPrimitive.RadioItem
      data-slot="menubar-radio-item"
      className={cn(
        menubarItemVariants(),
        'group/menubar-radio-item',
        'data-[state=checked]:bg-primary-container dark:data-[state=checked]:bg-on-primary',
        className,
      )}
      {...props}
    >
      <MenubarItemIndicator className="static translate-y-0">
        <RadioChecked aria-hidden="true" />
      </MenubarItemIndicator>

      <span className="group-data-[state=checked]/menubar-radio-item:hidden">
        <Radio
          className="text-outline-variant dark:text-outline"
          aria-hidden="true"
        />
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
  return (
    <MenubarPrimitive.SubTrigger
      data-slot="menubar-sub-trigger"
      className={cn(
        menubarItemVariants(),
        'data-[state=open]:bg-surface-variant/30 pe-2',
        className,
      )}
      {...props}
    >
      <Slot.Slottable>{children}</Slot.Slottable>

      <ArrowRight
        data-slot="menubar-sub-arrow"
        aria-hidden="true"
        className="text-outline-variant dark:text-outline ms-auto size-6 rtl:rotate-180"
      />
    </MenubarPrimitive.SubTrigger>
  )
}

export function MenubarSubContent({
  className,
  sideOffset = 4,
  style,
  children,
  asChild,
  forceMount,
  ...props
}: ComponentProps<typeof MenubarPrimitive.SubContent>) {
  return (
    <MenubarPortal forceMount={forceMount}>
      <MenubarPrimitive.SubContent
        data-slot="menubar-sub-content"
        sideOffset={sideOffset}
        forceMount={forceMount}
        {...props}
        asChild
      >
        <MenuSurface asChild={asChild} className={className} style={style}>
          {children}
        </MenuSurface>
      </MenubarPrimitive.SubContent>
    </MenubarPortal>
  )
}

export function MenubarLabel({
  className,
  ...props
}: ComponentProps<typeof MenubarPrimitive.Label>) {
  return (
    <MenubarPrimitive.Label
      data-slot="menubar-label"
      className={cn(
        'flex h-12 items-center justify-start gap-2 p-4 outline-hidden',
        className,
      )}
      {...props}
    />
  )
}

export function MenubarSeparator({
  className,
  ...props
}: ComponentProps<typeof MenubarPrimitive.Separator>) {
  return (
    <MenubarPrimitive.Separator
      data-slot="menubar-separator"
      className={cn('bg-outline-variant/50 h-px', className)}
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
      className={cn('fill-inverse-on-surface dark:fill-surface', className)}
      {...props}
    />
  )
}
