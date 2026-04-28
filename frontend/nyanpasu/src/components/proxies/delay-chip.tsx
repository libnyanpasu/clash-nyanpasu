import { ComponentProps } from 'react'
import { cn } from '@nyanpasu/utils'

export default function DelayChip({
  delay,
  className,
  ...props
}: Omit<ComponentProps<'span'>, 'children'> & {
  delay: number
}) {
  return (
    <span
      className={cn(
        'text-[10px]!',
        delay > 0 && 'text-green-500!',
        delay > 100 && 'text-yellow-500!',
        delay > 300 && 'text-orange-500!',
        delay > 500 && 'text-red-500!',
        className,
      )}
      data-slot="proxy-node-delay"
      {...props}
    >
      {delay} ms
    </span>
  )
}
