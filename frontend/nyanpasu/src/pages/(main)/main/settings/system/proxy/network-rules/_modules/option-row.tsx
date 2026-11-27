import type { ComponentProps, ReactNode } from 'react'
import { cn } from '@nyanpasu/utils'

/** A labelled option with its control at the end, as tonal list segment. */
export function OptionRow({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      className={cn(
        'flex min-h-14 items-center justify-between gap-4 py-2 pr-3 pl-4',
        'bg-surface-container-high rounded-extra-small',
        'first:rounded-t-large last:rounded-b-large',
        className,
      )}
      data-slot="option-row"
      {...props}
    />
  )
}

export function OptionRowLabel({
  icon,
  className,
  children,
  ...props
}: ComponentProps<'div'> & { icon: ReactNode }) {
  return (
    <div
      className={cn(
        'flex min-w-0 items-center gap-3',
        '[&>svg]:text-on-surface-variant [&>svg]:size-5 [&>svg]:shrink-0',
        className,
      )}
      data-slot="option-row-label"
      {...props}
    >
      {icon}

      <span className="truncate">{children}</span>
    </div>
  )
}
