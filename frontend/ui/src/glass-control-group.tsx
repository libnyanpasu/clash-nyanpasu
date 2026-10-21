import { cva } from 'class-variance-authority'
import type { ComponentProps } from 'react'
import { cn } from '@nyanpasu/utils'
import { GlassControlGroupContext, glassSurfaceVariants } from './glass-surface'

export const glassControlGroupVariants = cva([
  glassSurfaceVariants(),
  'text-on-surface-variant flex min-w-0 items-center gap-1 p-1 [&>button]:h-9',
])

export function GlassControlGroup({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <GlassControlGroupContext.Provider value>
      <div
        data-slot="glass-control-group"
        className={cn(glassControlGroupVariants(), className)}
        {...props}
      />
    </GlassControlGroupContext.Provider>
  )
}
