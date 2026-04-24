import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import { SettingsGroup, SettingsLabel } from '../_modules/settings-card'
import { SettingsTitle } from '../_modules/settings-title'
import LanguageSelector from './_modules/language-selector'
import ThemeColorConfig from './_modules/theme-color-config'
import ThemeModeSelector from './_modules/theme-mode-selector'

export const Route = createFileRoute('/(main)/main/settings/user-interface')({
  component: RouteComponent,
  head: () => ({
    meta: [
      {
        title: m.settings_user_interface_title(),
      },
    ],
  }),
})

const LanguageSettings = () => {
  return (
    <div data-slot="language-settings-container">
      <SettingsLabel>
        {m.settings_user_interface_language_group()}
      </SettingsLabel>

      <SettingsGroup>
        <LanguageSelector />
      </SettingsGroup>
    </div>
  )
}

const ThemeModeSettings = () => {
  return (
    <div data-slot="theme-mode-settings-container">
      <SettingsLabel>
        {m.settings_user_interface_theme_mode_group()}
      </SettingsLabel>

      <SettingsGroup>
        <ThemeModeSelector />

        <ThemeColorConfig />
      </SettingsGroup>
    </div>
  )
}

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_user_interface_title()}</SettingsTitle>

      <div className="space-y-4 px-4 pb-4">
        <LanguageSettings />

        <ThemeModeSettings />
      </div>
    </>
  )
}
