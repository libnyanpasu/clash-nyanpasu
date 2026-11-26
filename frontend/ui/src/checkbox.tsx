import CheckRounded from '~icons/material-symbols/check-rounded'
import { Checkbox as CheckboxPrimitive } from 'radix-ui'
import { ComponentProps } from 'react'
import { cn } from '@nyanpasu/utils'

export function Checkbox({
  className,
  ...props
}: ComponentProps<typeof CheckboxPrimitive.Root>) {
  return (
    <CheckboxPrimitive.Root
      data-slot="checkbox"
      className={cn(
        'inline-grid size-5 shrink-0 place-content-center rounded-sm',
        'border-outline cursor-pointer border-2 transition-colors',
        'data-[state=checked]:border-primary data-[state=checked]:bg-primary',
        'data-[state=checked]:text-on-primary',
        'focus-visible:outline-primary outline-none focus-visible:outline-2',
        'focus-visible:outline-offset-2',
        'disabled:cursor-not-allowed disabled:opacity-38',
        className,
      )}
      {...props}
    >
      <CheckboxPrimitive.Indicator data-slot="checkbox-indicator">
        <CheckRounded className="size-4" />
      </CheckboxPrimitive.Indicator>
    </CheckboxPrimitive.Root>
  )
}
