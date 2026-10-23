import { type ComponentType, type ReactNode } from 'react'
import { SettingsCardHeader } from '../../../../_modules/settings-card'

export default function AboutCardHeader({
  icon: Icon,
  children,
}: {
  icon: ComponentType<{ className?: string }>
  children: ReactNode
}) {
  return (
    <SettingsCardHeader data-slot="about-card-header">
      <Icon className="text-primary size-6 shrink-0" />

      <h2 className="text-lg font-medium">{children}</h2>
    </SettingsCardHeader>
  )
}
