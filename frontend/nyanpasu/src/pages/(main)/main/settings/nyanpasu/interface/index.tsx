import { CUSTOM_CSS_PRESETS } from '@/assets/custom-css-presets'
import { RequireCapability } from '@/components/auth/capabilities'
import { ReadOnlyScope } from '@/components/auth/read-only-scope'
import { PageTitle } from '@/components/page-title'
import useCustomCss from '@/hooks/use-custom-css'
import { m } from '@/paraglide/messages'
import { PlatformDesktop, usePlatform } from '@nyanpasu/platform/react'
import { createFileRoute } from '@tanstack/react-router'
import { SettingsLinkRow } from '../../_modules/settings-link-row'
import { SettingsGroup, SettingsLabel } from '../../../_modules/settings-card'
import LanguageSelector from './_modules/language-selector'
import ProfileFileOpenerSelector from './_modules/profile-file-opener-selector'
import ThemeColorConfig from './_modules/theme-color-config'
import ThemeModeSelector from './_modules/theme-mode-selector'

export const Route = createFileRoute(
  '/(main)/main/settings/nyanpasu/interface/',
)({
  component: RouteComponent,
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

const FileHandlingSettings = () => {
  return (
    <PlatformDesktop>
      <ReadOnlyScope permissions={['system:write']}>
        <div data-slot="file-handling-settings-container">
          <SettingsLabel>
            {m.settings_user_interface_file_handling_group()}
          </SettingsLabel>
          <SettingsGroup>
            <ProfileFileOpenerSelector />
          </SettingsGroup>
        </div>
      </ReadOnlyScope>
    </PlatformDesktop>
  )
}

const CustomCssPresetsEntry = () => {
  const customCss = useCustomCss()
  const { clientOs } = usePlatform()

  const enabled = CUSTOM_CSS_PRESETS.filter(
    (preset) =>
      (preset.platform !== 'macos' || clientOs === 'macos') &&
      customCss.state.enabledPresets.includes(preset.id),
  ).length

  return (
    <div data-slot="custom-css-presets-section">
      <SettingsLabel>
        {m.settings_user_interface_custom_css_presets_group()}
      </SettingsLabel>

      <SettingsGroup>
        <SettingsLinkRow
          to="/main/settings/nyanpasu/interface/presets"
          title={m.settings_user_interface_custom_css_presets_group()}
          description={m.settings_user_interface_custom_css_presets_entry_count(
            { count: enabled },
          )}
          data-slot="custom-css-presets-entry"
        />
      </SettingsGroup>
    </div>
  )
}

const CustomCssItemsEntry = () => {
  const customCss = useCustomCss()

  return (
    <div data-slot="custom-css-items-section">
      <SettingsLabel>
        {m.settings_user_interface_custom_css_group()}
      </SettingsLabel>

      <SettingsGroup>
        <SettingsLinkRow
          to="/main/settings/nyanpasu/interface/custom-css"
          title={m.settings_user_interface_custom_css_group()}
          description={m.settings_user_interface_custom_css_entry_count({
            count: customCss.state.items.length,
          })}
          data-slot="custom-css-items-entry"
        />
      </SettingsGroup>
    </div>
  )
}

const CustomCssEntries = () => {
  return (
    <RequireCapability permission="settings:read">
      <CustomCssPresetsEntry />

      <CustomCssItemsEntry />
    </RequireCapability>
  )
}

function RouteComponent() {
  return (
    <>
      <PageTitle />

      <ReadOnlyScope permissions={['settings:write']}>
        <div className="space-y-4 px-4 pb-4">
          <LanguageSettings />

          <ThemeModeSettings />

          <FileHandlingSettings />

          <CustomCssEntries />
        </div>
      </ReadOnlyScope>
    </>
  )
}
