import { ComponentProps } from 'react'
import HighlightText from '@/components/ui/highlight-text'
import { cn } from '@nyanpasu/utils'

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
        'bg-surface-variant/50 text-on-surface-variant inline-flex shrink-0 items-center rounded-md px-2 py-1 font-sans text-[11px] leading-none font-semibold tracking-wide uppercase',
        childrenLower === 'info' &&
          'bg-primary-container text-on-primary-container',
        (childrenLower === 'warn' || childrenLower === 'warning') &&
          'bg-tertiary-container text-on-tertiary-container',
        (childrenLower === 'error' || childrenLower === 'fatal') &&
          'bg-error-container text-on-error-container',
        className,
      )}
      {...props}
    >
      <HighlightText searchText={searchText}>
        {childrenLower === 'warning' ? 'warn' : children}
      </HighlightText>
    </div>
  )
}
