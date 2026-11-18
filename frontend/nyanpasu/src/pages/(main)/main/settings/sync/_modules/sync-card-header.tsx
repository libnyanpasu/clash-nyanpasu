import { type ComponentType, type ReactNode } from 'react'
import { SettingsCardHeader } from '../../../_modules/settings-card'

/** A settings card header with the section icon. */
export default function SyncCardHeader({
  icon: Icon,
  children,
}: {
  icon: ComponentType<{ className?: string }>
  children: ReactNode
}) {
  return (
    <SettingsCardHeader data-slot="sync-card-header">
      <Icon className="text-primary size-6 shrink-0" />

      <h2 className="text-lg font-medium">{children}</h2>
    </SettingsCardHeader>
  )
}
