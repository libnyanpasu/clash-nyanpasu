import React from 'react'
import { cn } from '../../utils/cn'

type ButtonVariant =
  | 'filled'
  | 'outlined'
  | 'text'
  | 'filled-tonal'
  | 'elevated'
type ButtonSize = 'default' | 'sm' | 'lg' | 'icon'

const getVariantClasses = (variant: ButtonVariant): string => {
  switch (variant) {
    case 'filled':
      return 'bg-primary text-on-primary shadow hover:shadow-md'
    case 'outlined':
      return 'border border-outline bg-transparent text-primary hover:bg-primary-container hover:text-on-primary-container'
    case 'text':
      return 'bg-transparent text-primary hover:bg-primary-container hover:text-on-primary-container'
    case 'filled-tonal':
      return 'bg-secondary-container text-on-secondary-container hover:shadow-sm'
    case 'elevated':
      return 'bg-surface-container-low text-on-surface shadow-sm hover:shadow-md'
    default:
      return 'bg-primary text-on-primary shadow hover:shadow-md'
  }
}

const getSizeClasses = (size: ButtonSize): string => {
  switch (size) {
    case 'sm':
      return 'h-8 px-4 py-1.5 text-xs'
    case 'lg':
      return 'h-12 px-8 py-3'
    case 'icon':
      return 'h-10 w-10'
    case 'default':
    default:
      return 'h-10 px-6 py-2'
  }
}

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant
  size?: ButtonSize
  asChild?: boolean
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  (
    {
      className,
      variant = 'filled',
      size = 'default',
      asChild = false,
      children,
      ...props
    },
    ref,
  ) => {
    const baseClasses =
      'inline-flex items-center justify-center whitespace-nowrap rounded-full text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50'
    const variantClasses = getVariantClasses(variant)
    const sizeClasses = getSizeClasses(size)

    if (asChild) {
      // For asChild functionality, we'd need @radix-ui/react-slot
      // For now, just render as button
      console.warn('asChild prop requires @radix-ui/react-slot dependency')
    }

    return (
      <button
        className={cn(baseClasses, variantClasses, sizeClasses, className)}
        ref={ref}
        {...props}
      >
        {children}
      </button>
    )
  },
)
Button.displayName = 'Button'

export { Button }
