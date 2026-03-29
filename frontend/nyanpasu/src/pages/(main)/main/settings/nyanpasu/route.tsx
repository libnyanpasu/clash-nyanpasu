import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import {
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../_modules/settings-card'
import { SettingsTitle } from '../_modules/settings-title'
import LogFileConfig from './_modules/log-file-config'
import LogLevelSelector from './_modules/log-level-selector'

export const Route = createFileRoute('/(main)/main/settings/nyanpasu')({
  component: RouteComponent,
})

const AppSettings = () => {
  return (
    <div data-slot="app-settings-container">
      <SettingsLabel>{m.settings_nyanpasu_logs()}</SettingsLabel>

      <SettingsGroup>
        <SettingsCard>
          <SettingsCardContent>
            <LogLevelSelector />
          </SettingsCardContent>
        </SettingsCard>

        <LogFileConfig />
      </SettingsGroup>
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
