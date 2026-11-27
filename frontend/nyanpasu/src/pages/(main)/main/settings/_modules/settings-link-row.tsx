import { type ReactNode } from 'react'
import { ItemAction } from '@nyanpasu/ui/item-group'
import { Link, type LinkProps } from '@tanstack/react-router'

/**
 * A settings section's entry row: one link to the page that owns the section's
 * controls, built on `@nyanpasu/ui/item-group` for strict left alignment,
 * full-row hover, and ripple feedback.
 */
export function SettingsLinkRow({
  to,
  params,
  search,
  leading,
  title,
  description,
  disabled,
  'data-slot': slot,
}: {
  to: LinkProps['to']
  params?: LinkProps['params']
  search?: LinkProps['search']
  leading?: ReactNode
  title: string
  description?: string
  disabled?: boolean
  /** The entry's own styling slot; the row root keeps `settings-link-row`. */
  'data-slot'?: string
}) {
  return (
    <div
      data-slot="settings-link-row"
      className="rounded-extra-large overflow-hidden *:rounded-[inherit]!"
    >
      <ItemAction
        asChild
        leading={leading}
        title={title}
        description={description}
        disabled={disabled}
      >
        <Link to={to} params={params} search={search} data-slot={slot} />
      </ItemAction>
    </div>
  )
}
