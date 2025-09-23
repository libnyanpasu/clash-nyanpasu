import React from 'react'
import { cn } from '../../utils/cn'
import { Switch, SwitchProps } from './switch'

type BaseSwitchProps = Omit<SwitchProps, 'onChange' | 'onCheckedChange'> & {
  onCheckedChange?: (checked: boolean) => void
}

interface LoadingSwitchProps extends BaseSwitchProps {
  loading?: boolean
  // MUI-style onChange signature for easier migration compatibility
  onChange?: (
    event: React.ChangeEvent<HTMLInputElement> | undefined,
    checked: boolean,
  ) => void | Promise<void>
}

/**
 * Loading Switch component that shows a loading indicator when loading is true
 * Replaces the MUI-based LoadingSwitch with Radix UI + Tailwind implementation
 *
 * @example
 * <LoadingSwitch
 *   loading={loading}
 *   onChange={handleChange}
 *   {...switchProps}
 * />
 *
 * Support loading status with Material Design 3 styling.
 *
 * @author keiko233 <i@elaina.moe>
 * @copyright LibNyanpasu org. 2024
 */
export const LoadingSwitch = React.forwardRef<
  React.ElementRef<typeof Switch>,
  LoadingSwitchProps
>(
  (
    {
      loading,
      checked,
      disabled,
      className,
      size = 'medium',
      onChange,
      onCheckedChange,
      ...props
    },
    ref,
  ) => {
    const containerClasses = cn('relative inline-flex items-center', className)

    const loadingClasses = cn(
      'absolute inset-0 flex items-center justify-center pointer-events-none',
      'transition-opacity duration-200',
      loading ? 'opacity-100' : 'opacity-0',
    )

    const spinnerSize = size === 'small' ? 'w-3 h-3' : 'w-4 h-4'

    return (
      <div className={containerClasses}>
        {loading && (
          <div className={loadingClasses}>
            <div
              className={cn(
                'animate-spin rounded-full border-2 border-current border-t-transparent',
                spinnerSize,
                checked ? 'text-on-primary' : 'text-primary',
              )}
              aria-labelledby={props.id}
            />
          </div>
        )}
        <Switch
          disabled={loading || disabled}
          checked={checked}
          size={size}
          onCheckedChange={(next: boolean) => {
            // bridge to MUI-style onChange if provided
            if (onChange) onChange(undefined, !!next)
            onCheckedChange?.(!!next)
          }}
          className={cn(
            loading && 'opacity-50',
            'transition-opacity duration-200',
          )}
          ref={ref}
          {...props}
        />
      </div>
    )
  },
)

LoadingSwitch.displayName = 'LoadingSwitch'

export default LoadingSwitch
