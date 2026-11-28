import Check from '~icons/material-symbols/check-rounded'
import { cva } from 'class-variance-authority'
import { RadioGroup as RadioGroupPrimitive } from 'radix-ui'
import { ComponentProps } from 'react'
import { cn } from '@nyanpasu/utils'

export type ColorSchemeSwatchColors = {
  primary: string
  secondary: string
  tertiary: string
  surface: string
}

const swatchVariants = cva([
  'group relative grid size-16 shrink-0 cursor-pointer place-items-center',
  'bg-surface-container-highest outline-primary outline-offset-2',
  'transition-[border-radius,outline-offset] duration-300 ease-[cubic-bezier(0.34,1.56,0.64,1)]',
  'focus-visible:outline-[3px] motion-reduce:transition-none',
  'data-[state=checked]:rounded-exl data-[state=checked]:outline-2',
  'data-[state=unchecked]:rounded-large',
  'data-disabled:pointer-events-none data-disabled:opacity-38',
  // Hover state layer.
  'before:bg-on-surface before:absolute before:inset-0 before:rounded-[inherit] before:opacity-0',
  'before:transition-opacity before:duration-150 hover:before:opacity-8 motion-reduce:before:transition-none',
])

const swatchDiscVariants = cva([
  'ring-surface relative size-10 overflow-hidden rounded-full ring-2',
  'grid grid-cols-2 grid-rows-2',
])

const swatchCheckVariants = cva([
  'bg-primary text-on-primary absolute grid size-6 place-items-center rounded-full',
  'scale-0 transition-transform duration-200 ease-[cubic-bezier(0.2,0,0,1)] motion-reduce:transition-none',
  'group-data-[state=checked]:scale-100',
])

export function ColorSchemeSwatchGroup({
  className,
  ...props
}: ComponentProps<typeof RadioGroupPrimitive.Root>) {
  return (
    <RadioGroupPrimitive.Root
      data-slot="color-scheme-swatch-group"
      className={cn('flex flex-wrap items-center gap-3 p-1', className)}
      {...props}
    />
  )
}

export function ColorSchemeSwatch({
  value,
  label,
  colors,
  className,
  ...props
}: Omit<
  ComponentProps<typeof RadioGroupPrimitive.Item>,
  'children' | 'value'
> & {
  value: string
  label: string
  colors: ColorSchemeSwatchColors
}) {
  return (
    <RadioGroupPrimitive.Item
      value={value}
      aria-label={label}
      title={label}
      data-slot="color-scheme-swatch"
      className={cn(swatchVariants(), className)}
      {...props}
    >
      <span
        aria-hidden
        data-slot="color-scheme-swatch-disc"
        className={swatchDiscVariants()}
        style={{ backgroundColor: colors.surface }}
      >
        <span
          data-slot="color-scheme-swatch-primary"
          className="col-span-2"
          style={{ backgroundColor: colors.primary }}
        />
        <span
          data-slot="color-scheme-swatch-secondary"
          style={{ backgroundColor: colors.secondary }}
        />
        <span
          data-slot="color-scheme-swatch-tertiary"
          style={{ backgroundColor: colors.tertiary }}
        />
      </span>

      <span
        aria-hidden
        data-slot="color-scheme-swatch-check"
        className={swatchCheckVariants()}
      >
        <Check className="size-4" />
      </span>
    </RadioGroupPrimitive.Item>
  )
}
