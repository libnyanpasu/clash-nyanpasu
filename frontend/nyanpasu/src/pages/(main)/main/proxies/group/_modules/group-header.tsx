import { ComponentProps } from 'react'
import { BackButton } from '@/components/page-title'
import { cn } from '@nyanpasu/utils'

export default function GroupHeader({
  children,
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    // The horizontal padding sits on the root so the header row and the
    // content under it share their edges, also while the scrollbar shows.
    <div
      className={cn(
        'sticky top-0 z-10 transition-[padding] duration-500',
        'bg-surface-container dark:bg-surface',
        'flex flex-col',
        'pr-4 pl-2 md:pl-4',
        'group-data-[scroll-direction=down]/proxies-content:pr-6',
        'group-data-[scroll-direction=down]/proxies-content:pl-3',
        'group-data-[scroll-direction=down]/proxies-content:md:pl-6',
        className,
      )}
      {...props}
    >
      <div
        className="relative flex items-center gap-1 py-2 md:py-4"
        data-slot="proxies-group-header-row"
      >
        <BackButton
          className="shrink-0"
          data-slot="proxies-group-back-button"
        />

        {children}
      </div>
    </div>
  )
}
