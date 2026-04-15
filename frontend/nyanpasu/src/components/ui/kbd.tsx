import { ComponentProps } from 'react'
import { cn } from '@nyanpasu/ui'

export type KbdProps = ComponentProps<'kbd'>

export const Kbd = ({ className, ...props }: KbdProps) => {
  return (
    <kbd
      className={cn(
        'inline-flex items-center justify-center',
        'rounded-3xl border border-solid px-2 py-0.5',
        'font-mono text-xs! font-medium',
        'bg-transparent-fallback-surface dark:bg-transparent-fallback-surface-variant',
        'border-zinc-400/50 dark:border-zinc-500/50',
        'text-on-surface dark:text-on-surface',
        className,
      )}
      {...props}
    />
  )
}
