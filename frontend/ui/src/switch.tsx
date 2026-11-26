import { Switch as SwitchPrimitives } from 'radix-ui'
import React, { ComponentProps } from 'react'
import { cn } from '@nyanpasu/utils'
import { CircularProgress } from './progress'

export const Switch = ({
  className,
  children,
  loading,
  ...props
}: React.ComponentProps<typeof SwitchPrimitives.Root> & {
  loading?: boolean
}) => {
  return (
    <SwitchPrimitives.Root
      className={cn(
        'peer',
        'inline-flex h-8 w-14 shrink-0 cursor-pointer items-center rounded-full',
        'focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:outline-none',
        'disabled:cursor-not-allowed disabled:opacity-50',
        'border-2 transition-colors',
        'data-[state=unchecked]:border-outline data-[state=checked]:border-transparent',
        'data-[state=checked]:bg-primary data-[state=unchecked]:bg-surface-container-highest',
        className,
      )}
      {...props}
    >
      <SwitchPrimitives.Thumb
        className={cn(
          'group',
          'pointer-events-none block',
          'rounded-full shadow-lg ring-0 transition-all duration-200 ease-in-out',
          'data-[state=checked]:bg-on-primary data-[state=unchecked]:bg-outline',
          'data-[state=checked]:size-6 data-[state=unchecked]:size-4',
          'data-[state=checked]:translate-x-6.5 data-[state=unchecked]:translate-x-1.5',
        )}
      >
        {loading && (
          <span
            className="grid h-full w-full place-items-center"
            data-slot="switch-thumb-loading"
          >
            <CircularProgress
              className="text-surface-container-highest group-data-[state=checked]:text-primary group-data-[state=checked]:size-4 group-data-[state=unchecked]:size-2.5"
              data-slot="switch-thumb-loading-circular-progress"
              indeterminate
            />
          </span>
        )}
      </SwitchPrimitives.Thumb>

      {children}
    </SwitchPrimitives.Root>
  )
}

export function SwitchItem({
  children,
  className,
  ...props
}: ComponentProps<typeof Switch>) {
  return (
    <div
      className={cn(
        'flex h-16 w-full items-center justify-between gap-2',
        'bg-surface-container-low',
        'rounded-xl',
        'p-4',
        className,
      )}
    >
      {children}

      <Switch {...props} />
    </div>
  )
}
