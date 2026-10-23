import type { ComponentProps, ReactNode } from 'react'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { cn } from '@nyanpasu/utils'

/**
 * One column of the page. From the wide layout on it fills its share of the
 * height and scrolls on its own; below that the columns stack and the page
 * scrolls them as one region, so the inner area simply grows.
 */
export default function Column({
  className,
  children,
  ...props
}: ComponentProps<'section'>) {
  return (
    <section
      className={cn('flex min-h-0 min-w-0 flex-col @[56rem]:flex-1', className)}
      {...props}
    >
      <ScrollArea
        data-slot="profile-global-column-scroll-area"
        className="min-h-0 flex-1"
      >
        <div className="flex min-w-0 flex-col gap-4 px-3 py-3 md:px-5 md:pt-4 md:pb-6">
          {children}
        </div>
      </ScrollArea>
    </section>
  )
}

/** The title of a column with what sits at its end. */
export function ColumnHead({
  id,
  title,
  children,
}: {
  id: string
  title: string
  children?: ReactNode
}) {
  return (
    <div className="flex min-h-8 items-center gap-2 pl-4">
      <h2
        id={id}
        className="text-on-surface min-w-0 flex-1 truncate text-sm font-semibold"
      >
        {title}
      </h2>

      {children}
    </div>
  )
}
