import BoxOutlineRounded from '~icons/material-symbols/box-outline-rounded'
import { cva, type VariantProps } from 'class-variance-authority'
import { type ComponentProps, type ComponentType, type ReactNode } from 'react'
import { cn } from '@nyanpasu/utils'

const emptyStateVariants = cva(
  'flex min-h-0 min-w-0 flex-col items-center justify-center text-center [&>[data-slot=empty-state-icon]]:shrink-0',
  {
    variants: {
      tone: {
        muted: 'text-surface-variant',
        normal: 'text-on-surface-variant',
      },
      size: {
        default: 'gap-4 p-6 text-sm [&>[data-slot=empty-state-icon]]:size-16',
        compact: 'gap-2 p-3 text-xs [&>[data-slot=empty-state-icon]]:size-8',
      },
    },
    defaultVariants: { size: 'default', tone: 'muted' },
  },
)

export type EmptyStateProps = ComponentProps<'div'> &
  VariantProps<typeof emptyStateVariants> & {
    message: ReactNode
    description?: ReactNode
    icon?: ComponentType<ComponentProps<'svg'>>
  }

/**
 * Connections-style empty content; the caller owns placement and available height.
 * Use the normal tone on cards, dialogs and translucent surfaces for readable contrast.
 */
export function EmptyState({
  message,
  description,
  icon: Icon = BoxOutlineRounded,
  size,
  tone,
  className,
  children,
  ...props
}: EmptyStateProps) {
  return (
    <div
      data-slot="empty-state"
      role="status"
      className={cn(emptyStateVariants({ size, tone }), className)}
      {...props}
    >
      <Icon data-slot="empty-state-icon" aria-hidden="true" />
      <div
        data-slot="empty-state-message"
        className="max-w-full wrap-break-word"
      >
        {message}
      </div>
      {description && (
        <div
          data-slot="empty-state-description"
          className="max-w-full text-xs wrap-break-word"
        >
          {description}
        </div>
      )}
      {children && (
        <div
          data-slot="empty-state-actions"
          className="text-on-surface flex flex-wrap items-center justify-center gap-2"
        >
          {children}
        </div>
      )}
    </div>
  )
}
