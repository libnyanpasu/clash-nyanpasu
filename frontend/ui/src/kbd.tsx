import { ComponentProps } from 'react'
import { cn } from '@nyanpasu/utils'

export type KbdProps = ComponentProps<'kbd'>

export const Kbd = ({ className, ...props }: KbdProps) => {
  return (
    <kbd
      className={cn(
        'inline-flex items-center justify-center',
        'rounded-extra-large border border-solid px-2 py-0.5',
        'font-mono text-xs! font-medium',
        'bg-surface-container-highest border-outline-variant text-on-surface-variant',
        className,
      )}
      {...props}
    />
  )
}
