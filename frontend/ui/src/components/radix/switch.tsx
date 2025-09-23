import React from 'react'
import * as SwitchPrimitive from '@radix-ui/react-switch'
import { cn } from '../../utils/cn'

export interface SwitchProps
  extends React.ComponentPropsWithoutRef<typeof SwitchPrimitive.Root> {
  size?: 'small' | 'medium'
  className?: string
  checked?: boolean
  disabled?: boolean
  id?: string
  onCheckedChange?: (checked: boolean) => void
}

const Switch = React.forwardRef<
  React.ElementRef<typeof SwitchPrimitive.Root>,
  SwitchProps
>(({ className, size = 'medium', ...props }, ref) => {
  const sizeClasses = {
    small: {
      root: 'h-6 w-11',
      thumb:
        'h-4 w-4 data-[state=checked]:translate-x-5 data-[state=unchecked]:translate-x-0.5',
    },
    medium: {
      root: 'h-8 w-14',
      thumb:
        'h-6 w-6 data-[state=checked]:translate-x-6 data-[state=unchecked]:translate-x-1',
    },
  }

  return (
    <SwitchPrimitive.Root
      className={cn(
        'peer focus-visible:ring-ring focus-visible:ring-offset-background inline-flex shrink-0 cursor-pointer items-center rounded-full border-2 border-transparent shadow-sm transition-colors focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50',
        'data-[state=checked]:bg-primary data-[state=unchecked]:bg-outline',
        sizeClasses[size].root,
        className,
      )}
      {...props}
      ref={ref}
    >
      <SwitchPrimitive.Thumb
        className={cn(
          'bg-background pointer-events-none block rounded-full shadow-lg ring-0 transition-transform',
          'data-[state=checked]:bg-on-primary data-[state=unchecked]:bg-surface',
          sizeClasses[size].thumb,
        )}
      />
    </SwitchPrimitive.Root>
  )
})
Switch.displayName = SwitchPrimitive.Root.displayName

export { Switch }
