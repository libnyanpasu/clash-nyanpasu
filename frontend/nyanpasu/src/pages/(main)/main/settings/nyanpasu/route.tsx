import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import { SettingsLabel } from '../_modules/settings-card'
import { SettingsTitle } from '../_modules/settings-title'
import LogFileConfig from './_modules/log-file-config'

export const Route = createFileRoute('/(main)/main/settings/nyanpasu')({
  component: RouteComponent,
})

const AppSettings = () => {
  return (
    <div data-slot="app-settings-container">
      <SettingsLabel>{m.settings_label_nyanpasu()}</SettingsLabel>

      <LogFileConfig />
    </div>
  )
}

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_label_nyanpasu()}</SettingsTitle>

      <div className="space-y-4 px-4 pb-4">
        <AppSettings />
      </div>
    </>
  )
}
