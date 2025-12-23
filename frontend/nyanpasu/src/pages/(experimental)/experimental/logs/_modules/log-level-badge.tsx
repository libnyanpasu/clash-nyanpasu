import { ComponentProps } from 'react'
import { cn } from '@nyanpasu/ui'

export default function LogLevelBadge({
  className,
  ...props
}: ComponentProps<'div'> & { children: string }) {
  const childrenLower = props.children?.toLowerCase()

  return (
    <div
      className={cn(
        'inline-block rounded-full px-2 py-1 font-semibold uppercase',
        childrenLower === 'info' && 'text-blue-500',
        childrenLower === 'warn' && 'text-yellow-500',
        childrenLower === 'error' && 'text-red-500',
        className,
      )}
      {...props}
    />
  )
}
