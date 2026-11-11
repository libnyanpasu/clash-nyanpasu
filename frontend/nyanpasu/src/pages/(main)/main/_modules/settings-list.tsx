import { ComponentProps, ReactNode, Ref } from 'react'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import {
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
} from './settings-card'

/** Shared spacing for a settings list, its label and trailing actions. */
export function SettingsList({
  className,
  ...props
}: ComponentProps<'section'>) {
  return (
    <section
      className={cn('flex min-w-0 flex-col gap-3', className)}
      data-slot="settings-list"
      {...props}
    />
  )
}

/** Rows and the explanatory item share the SettingsGroup corner treatment. */
export function SettingsListContent({
  empty,
  emptyMessage = m.settings_list_empty(),
  description,
  children,
  ...props
}: ComponentProps<typeof SettingsGroup> & {
  empty: boolean
  emptyMessage?: ReactNode
  description?: ReactNode
}) {
  return (
    <SettingsGroup {...props}>
      {empty ? (
        <SettingsCard data-slot="settings-list-empty">
          <SettingsCardContent>
            <EmptyState
              tone="normal"
              className="min-h-40"
              message={emptyMessage}
            />
          </SettingsCardContent>
        </SettingsCard>
      ) : (
        children
      )}

      {description && (
        <SettingsCard data-slot="settings-list-description">
          <SettingsCardContent className="text-on-surface-variant gap-2 text-sm">
            {description}
          </SettingsCardContent>
        </SettingsCard>
      )}
    </SettingsGroup>
  )
}

/** Standard row surface and spacing; controls and labels remain explicit. */
export function SettingsListItem({
  className,
  ...props
}: ComponentProps<typeof SettingsCard> & { ref?: Ref<HTMLDivElement> }) {
  return (
    <SettingsCard
      className={cn('flex min-w-0 items-center gap-4 px-5 py-6', className)}
      data-slot="settings-list-item"
      {...props}
    />
  )
}

export function SettingsListActions({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn('flex justify-end', className)}
      data-slot="settings-list-actions"
      {...props}
    />
  )
}
