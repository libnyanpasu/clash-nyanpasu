import * as React from 'react'
import { cn } from '@nyanpasu/ui'
import * as TooltipPrimitive from '@radix-ui/react-tooltip'

export function TooltipProvider({
  delayDuration = 0,
  ...props
}: React.ComponentProps<typeof TooltipPrimitive.Provider>) {
  return (
    <TooltipPrimitive.Provider
      data-slot="tooltip-provider"
      delayDuration={delayDuration}
      {...props}
    />
  )
}

export function Tooltip({
  ...props
}: React.ComponentProps<typeof TooltipPrimitive.Root>) {
  return (
    <TooltipProvider>
      <TooltipPrimitive.Root data-slot="tooltip" {...props} />
    </TooltipProvider>
  )
}

export function TooltipTrigger({
  ...props
}: React.ComponentProps<typeof TooltipPrimitive.Trigger>) {
  return <TooltipPrimitive.Trigger data-slot="tooltip-trigger" {...props} />
}

export function TooltipContent({
  className,
  sideOffset = 0,
  children,
  disableArrow = false,
  ...props
}: React.ComponentProps<typeof TooltipPrimitive.Content> & {
  disableArrow?: boolean
}) {
  return (
    <TooltipPrimitive.Portal>
      <TooltipPrimitive.Content
        data-slot="tooltip-content"
        sideOffset={sideOffset}
        className={cn(
          'bg-surface-variant text-on-surface',
          'z-50 w-fit min-w-12 text-center',
          'rounded-full px-3 py-1.5 text-xs text-balance',
          'shadow-outline/30 dark:shadow-surface-variant/20 shadow-sm',
          className,
        )}
        {...props}
      >
        {children}

        {!disableArrow && (
          <TooltipPrimitive.Arrow
            className={cn(
              'fill-surface-variant z-50',
              'h-2.5 w-4 translate-y-[-6px] rounded-xl',
            )}
          />
        )}
      </TooltipPrimitive.Content>
    </TooltipPrimitive.Portal>
  )
}
