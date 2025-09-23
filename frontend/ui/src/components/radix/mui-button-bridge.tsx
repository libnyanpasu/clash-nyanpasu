import React from 'react'
import { cn } from '../../utils/cn'
import { Button } from './button'

// A minimal compatibility bridge for common MUI Button props used in this codebase.
// Maps MUI variants to our Radix Button variants and supports an optional `loading` prop.
// This enables low-risk incremental migration by swapping imports from `@mui/material/Button`.

export type MUIVariant = 'text' | 'outlined' | 'contained'
export type MUISize = 'small' | 'medium' | 'large'

export interface MUIButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: MUIVariant
  size?: MUISize
  disabled?: boolean
  loading?: boolean
  startIcon?: React.ReactNode
  endIcon?: React.ReactNode
  // Accept MUI color prop for compatibility (not fully mapped yet)
  color?:
    | 'inherit'
    | 'primary'
    | 'secondary'
    | 'success'
    | 'error'
    | 'info'
    | 'warning'
}

const mapVariant = (variant?: MUIVariant) => {
  switch (variant) {
    case 'contained':
      return 'filled' as const
    case 'outlined':
      return 'outlined' as const
    case 'text':
      return 'text' as const
    default:
      return 'filled' as const
  }
}

const mapSize = (size?: MUISize) => {
  switch (size) {
    case 'small':
      return 'sm' as const
    case 'large':
      return 'lg' as const
    case 'medium':
    default:
      return 'default' as const
  }
}

export const MUIButton = React.forwardRef<HTMLButtonElement, MUIButtonProps>(
  (
    {
      className,
      variant = 'contained',
      size = 'medium',
      disabled,
      loading,
      startIcon,
      endIcon,
      children,
      ...props
    },
    ref,
  ) => {
    const radixVariant = mapVariant(variant)
    const radixSize = mapSize(size)

    return (
      <Button
        variant={radixVariant}
        size={radixSize}
        disabled={disabled || loading}
        className={cn('relative', className)}
        ref={ref}
        {...props}
      >
        {/* Spinner overlay when loading */}
        {loading && (
          <span className="absolute inset-0 flex items-center justify-center">
            <span className="h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent" />
          </span>
        )}
        <span
          className={cn(
            'inline-flex items-center gap-2',
            loading && 'opacity-0',
          )}
        >
          {startIcon && (
            <span className="inline-flex items-center" aria-hidden>
              {startIcon}
            </span>
          )}
          <span>{children}</span>
          {endIcon && (
            <span className="inline-flex items-center" aria-hidden>
              {endIcon}
            </span>
          )}
        </span>
      </Button>
    )
  },
)
MUIButton.displayName = 'MUIButton'

export default MUIButton
