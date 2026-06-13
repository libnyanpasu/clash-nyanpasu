import { motion } from 'framer-motion'
import { cn } from '@nyanpasu/utils'
import {
  TooltipContent as TooltipContentPrimitive,
  Tooltip as TooltipPrimitive,
  TooltipProvider as TooltipProviderPrimitive,
  TooltipTrigger as TooltipTriggerPrimitive,
  type TooltipContentProps as TooltipContentPrimitiveProps,
  type TooltipProps as TooltipPrimitiveProps,
  type TooltipProviderProps as TooltipProviderPrimitiveProps,
  type TooltipTriggerProps as TooltipTriggerPrimitiveProps,
} from '../primitives/animate/tooltip'

export type TooltipProviderProps = TooltipProviderPrimitiveProps

export function TooltipProvider({
  openDelay = 0,
  ...props
}: TooltipProviderProps) {
  return <TooltipProviderPrimitive openDelay={openDelay} {...props} />
}

export type TooltipProps = TooltipPrimitiveProps

export function Tooltip({ sideOffset = 10, ...props }: TooltipProps) {
  return <TooltipPrimitive sideOffset={sideOffset} {...props} />
}

export type TooltipTriggerProps = TooltipTriggerPrimitiveProps

export function TooltipTrigger({ ...props }: TooltipTriggerProps) {
  return <TooltipTriggerPrimitive {...props} />
}

export type TooltipContentProps = Omit<
  TooltipContentPrimitiveProps,
  'asChild'
> & {
  children: React.ReactNode
  layout?: boolean | 'position' | 'size' | 'preserve-aspect'
}

export function TooltipContent({
  className,
  children,
  layout = 'preserve-aspect',
  ...props
}: TooltipContentProps) {
  return (
    <TooltipContentPrimitive
      className={cn(
        'z-50 w-fit rounded-full text-xs text-balance',
        'dark:text-on-surface',
        'backdrop-blur-lg',
        'bg-primary-container/20 dark:bg-primary/10',
        'dark:shadow-inverse-on-surface/30 shadow-on-primary-container/30 shadow-sm',
        className,
      )}
      {...props}
    >
      <motion.div className="overflow-hidden px-3 py-1.5 text-xs text-balance">
        <motion.div layout={layout}>{children}</motion.div>
      </motion.div>

      {/* <TooltipArrowPrimitive
        className={cn(
          'fill-mixed-background/30 size-3',
          // 'backdrop-blur',
          "data-[side='bottom']:translate-y-px",
          "data-[side='left']:-translate-x-px",
          "data-[side='right']:translate-x-px",
          "data-[side='top']:-translate-y-px",
        )}
        tipRadius={2}
      /> */}
    </TooltipContentPrimitive>
  )
}
