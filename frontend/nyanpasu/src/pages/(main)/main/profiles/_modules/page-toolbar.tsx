import type { ComponentProps, ReactNode } from 'react'
import { BackButton } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import type { LinkProps } from '@tanstack/react-router'

/**
 * The header of a full-height tool page: a mobile back link, a one-line title,
 * a context area and the page actions, on a 56px row with a hairline below.
 * It takes the place of `PageTitle` on a page that does not scroll as a whole,
 * so it does not stick or animate.
 */
export default function PageToolbar({
  title,
  backTo,
  context,
  actions,
  contextClassName,
  actionsClassName,
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
  /**
   * Layout of the two areas. A page whose controls need their own wrapping,
   * such as a second row on a narrow toolbar, makes an area `contents` and
   * orders its controls beside the title.
   */
  contextClassName?: string
  actionsClassName?: string
}) {
  return (
    <header
      className={cn(
        'flex flex-wrap items-center gap-x-2.5 gap-y-2',
        // 56px on one row; it only grows when the controls wrap.
        'min-h-14 shrink-0 py-2 pr-3 pl-4 md:pl-5',
        'shadow-[inset_0_-1px_0_var(--color-outline-variant)]',
        className,
      )}
      data-slot="page-toolbar"
      {...props}
    >
      <BackButton to={backTo} label={m.profile_back()} />

      <h1
        className="text-on-surface max-w-full min-w-0 truncate py-1 text-base leading-6 font-semibold"
        data-slot="page-toolbar-title"
      >
        {title}
      </h1>

      {context && (
        <div
          className={cn('flex min-w-0 items-center gap-2.5', contextClassName)}
          data-slot="page-toolbar-context"
        >
          {context}
        </div>
      )}

      {actions && (
        <div
          className={cn('ml-auto flex items-center gap-2.5', actionsClassName)}
          data-slot="page-toolbar-actions"
        >
          {actions}
        </div>
      )}
    </header>
  )
}
