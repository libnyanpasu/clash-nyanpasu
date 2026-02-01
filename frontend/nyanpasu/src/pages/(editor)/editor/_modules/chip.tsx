import { ComponentProps } from 'react'
import { cn } from '@nyanpasu/ui'

export default function Chip({ className, ...props }: ComponentProps<'span'>) {
  return (
    <span
      className={cn(
        'bg-primary-container rounded-full px-3 py-0.5 text-sm font-bold',
        className,
      )}
      {...props}
    />
  )
}
