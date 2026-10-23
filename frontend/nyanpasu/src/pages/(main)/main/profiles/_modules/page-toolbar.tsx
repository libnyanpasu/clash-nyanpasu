import type { ComponentProps, ReactNode } from 'react'
import { BackButton } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import type { LinkProps } from '@tanstack/react-router'

/**
 * The header of a full-height tool page: a mobile back link, a one-line title,
 * a free context area and the page actions. It takes the place of `PageTitle`
 * on a page that does not scroll as a whole, so it does not stick or animate.
 */
export default function PageToolbar({
  title,
  backTo,
  context,
  actions,
  className,
  ...props
}: Omit<ComponentProps<'header'>, 'title'> & {
  title: string
  /** Route the mobile back link navigates to. */
  backTo: LinkProps['to']
  /** Page state beside the title, such as what the page applies to. */
  context?: ReactNode
  /** Controls at the end of the toolbar. */
  actions?: ReactNode
}) {
  return (
    <header
      className={cn(
        'flex flex-wrap items-center gap-x-3 gap-y-2',
        'min-h-14 shrink-0 px-4 md:px-6',
        className,
      )}
      data-slot="page-toolbar"
      {...props}
    >
      <BackButton to={backTo} label={m.profile_back()} />

      <h1
        className="max-w-full min-w-0 truncate text-xl font-medium"
        data-slot="page-toolbar-title"
      >
        {title}
      </h1>

      {context && (
        <div className="min-w-0 flex-1" data-slot="page-toolbar-context">
          {context}
        </div>
      )}

      {actions && (
        <div
          className="ml-auto flex items-center gap-2"
          data-slot="page-toolbar-actions"
        >
          {actions}
        </div>
      )}
    </header>
  )
}
