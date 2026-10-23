import { cva } from 'class-variance-authority'
import type { ComponentProps, ReactNode } from 'react'
import { cn } from '@nyanpasu/utils'

/**
 * The heading of a stage: a small uppercase label, optionally numbered, with
 * a note at the end. It sits above a `StepGroup`.
 */
export function StageLabel({
  index,
  aside,
  className,
  children,
  ...props
}: Omit<ComponentProps<'h3'>, 'children'> & {
  index?: number
  /** Sits at the end of the label, such as what can be done to the stage. */
  aside?: ReactNode
  children: ReactNode
}) {
  return (
    <h3
      data-slot="stage-label"
      className={cn(
        'text-on-surface-variant flex items-center gap-2 px-4 pb-2',
        'text-[11px] leading-4 font-bold tracking-[0.06em] uppercase',
        className,
      )}
      {...props}
    >
      {index !== undefined && (
        <span
          data-slot="stage-label-index"
          className="bg-surface-variant text-on-surface-variant grid size-[18px] shrink-0 place-items-center rounded-full text-[10px]"
        >
          {index}
        </span>
      )}

      {children}

      {aside && (
        <span className="text-primary ml-auto font-medium tracking-normal normal-case">
          {aside}
        </span>
      )}
    </h3>
  )
}

/**
 * A grouped list of rows: 2px apart, 4px corners inside and 24px at the two
 * ends. A row marked `data-selected` is rounded on its own.
 */
export function StepGroup({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      data-slot="step-group"
      className={cn(
        'flex min-w-0 flex-col gap-0.5 *:rounded-sm *:transition-[border-radius]',
        '[&>:first-child]:rounded-t-3xl [&>:last-child]:rounded-b-3xl',
        '[&>[data-selected]]:rounded-2xl!',
        className,
      )}
      {...props}
    />
  )
}

/** A row of a `StepGroup`: a button, a link or a plain element. */
export const stepRowVariants = cva(
  [
    'group/row relative flex min-h-13 w-full min-w-0 items-center gap-3',
    'py-1.5 pr-3 pl-4 text-left',
    'bg-surface text-on-surface shadow-sm dark:shadow-none',
    'data-selected:bg-secondary-container data-selected:text-on-secondary-container',
    'outline-none focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-primary',
    // The state layer is a pseudo element, so the row keeps its own shadow.
    'before:pointer-events-none before:absolute before:inset-0 before:rounded-[inherit]',
    'before:bg-on-surface/8 before:opacity-0',
  ],
  {
    variants: {
      interactive: {
        true: 'cursor-pointer hover:before:opacity-100',
        false: '',
      },
    },
    defaultVariants: { interactive: false },
  },
)

/** The round leading icon of a row, a hint of what the row is. */
export function StepRowIcon({
  className,
  children,
  ...props
}: Omit<ComponentProps<'span'>, 'children'> & { children: ReactNode }) {
  return (
    <span
      data-slot="step-row-icon"
      aria-hidden
      className={cn(
        'bg-secondary-container text-on-secondary-container grid size-9 shrink-0 place-items-center rounded-full [&_svg]:size-5',
        'group-data-selected/row:bg-surface group-data-selected/row:text-on-surface',
        className,
      )}
      {...props}
    >
      {children}
    </span>
  )
}

/** The title and the supporting line of a row. */
export function StepRowText({
  title,
  sub,
  dim,
  className,
  ...props
}: Omit<ComponentProps<'span'>, 'title'> & {
  title: ReactNode
  sub?: ReactNode
  /** Mutes the title, for a row that only points somewhere else. */
  dim?: boolean
}) {
  return (
    <span
      data-slot="step-row-text"
      className={cn('flex min-w-0 flex-1 flex-col gap-px', className)}
      {...props}
    >
      <span
        data-slot="step-row-title"
        className={cn(
          'truncate text-sm leading-5 font-medium',
          dim && 'text-on-surface-variant',
        )}
      >
        {title}
      </span>

      {sub && (
        <span
          data-slot="step-row-sub"
          className={cn(
            'text-on-surface-variant truncate text-xs leading-4',
            'group-data-selected/row:text-on-secondary-container group-data-selected/row:opacity-80',
          )}
        >
          {sub}
        </span>
      )}
    </span>
  )
}
