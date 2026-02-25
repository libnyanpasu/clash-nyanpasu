import { ComponentProps } from 'react'
import HighlightText from '@/components/ui/highlight-text'
import { cn } from '@nyanpasu/ui'

export default function LogLevelBadge({
  className,
  searchText = '',
  children,
  ...props
}: ComponentProps<'div'> & { children: string; searchText?: string }) {
  const childrenLower = children?.toLowerCase()

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
    >
      <HighlightText searchText={searchText}>{children}</HighlightText>
    </div>
  )
}
