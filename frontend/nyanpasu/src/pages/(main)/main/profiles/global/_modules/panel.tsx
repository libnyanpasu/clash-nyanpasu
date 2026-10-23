import { useId, type ComponentProps, type ReactNode } from 'react'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { cn } from '@nyanpasu/utils'

/**
 * One column of the page: a fixed heading and toolbar above a region that
 * scrolls on its own. Below the wide layout the panels stack and the page
 * scrolls them as one region, so the inner area simply grows.
 */
export default function Panel({
  title,
  action,
  toolbar,
  className,
  children,
  ...props
}: Omit<ComponentProps<'section'>, 'title'> & {
  title: string
  /** Sits at the end of the heading, such as a hint or a button. */
  action?: ReactNode
  /** Controls between the heading and the scrolling region. */
  toolbar?: ReactNode
}) {
  const headingId = useId()

  return (
    <section
      aria-labelledby={headingId}
      data-slot="profile-global-panel"
      className={cn('flex min-h-0 min-w-0 flex-col gap-2', className)}
      {...props}
    >
      <header className="flex min-h-10 shrink-0 items-center justify-between gap-2">
        <h2
          id={headingId}
          className="text-on-primary-container px-3 text-sm font-medium"
        >
          {title}
        </h2>

        {action}
      </header>

      {toolbar}

      <ScrollArea
        data-slot="profile-global-panel-scroll-area"
        className="min-h-0 flex-1"
      >
        {children}
      </ScrollArea>
    </section>
  )
}
