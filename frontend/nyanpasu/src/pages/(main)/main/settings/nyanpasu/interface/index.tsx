import {
  ItemGroup,
  ItemSection,
  ItemSectionHeader,
  ItemSectionTitle,
} from '@nyanpasu/ui/item-group'
import { CUSTOM_CSS_PRESETS } from '@/assets/custom-css-presets'
import { RequireCapability } from '@/components/auth/capabilities'
import { ReadOnlyScope } from '@/components/auth/read-only-scope'
import { PageTitle } from '@/components/page-title'
import useCustomCss from '@/hooks/use-custom-css'
import { m } from '@/paraglide/messages'
import { PlatformDesktop, usePlatform } from '@nyanpasu/platform/react'
import { createFileRoute } from '@tanstack/react-router'
import { SettingsLinkRow } from '../../_modules/settings-link-row'
import LanguageSelector from './_modules/language-selector'
import ProfileFileOpenerSelector from './_modules/profile-file-opener-selector'
import ThemeColorEntry from './_modules/theme-color-entry'
import ThemeModeSelector from './_modules/theme-mode-selector'

export const Route = createFileRoute(
  '/(main)/main/settings/nyanpasu/interface/',
)({
  component: RouteComponent,
})

const LanguageSettings = () => {
  return (
    <ItemSection data-slot="language-settings-container">
      <ItemSectionHeader>
        <ItemSectionTitle>
          {m.settings_user_interface_language_group()}
        </ItemSectionTitle>
      </ItemSectionHeader>

      <ItemGroup>
        <LanguageSelector />
      </ItemGroup>
    </ItemSection>
  )
}

const ThemeModeSettings = () => {
  return (
    <ItemSection data-slot="theme-mode-settings-container">
      <ItemSectionHeader>
        <ItemSectionTitle>
          {m.settings_user_interface_theme_mode_group()}
        </ItemSectionTitle>
      </ItemSectionHeader>

      <ItemGroup>
        <ThemeModeSelector />

        <ThemeColorEntry />
      </ItemGroup>
    </ItemSection>
  )
}

const FileHandlingSettings = () => {
  return (
    <PlatformDesktop>
      <ReadOnlyScope permissions={['system:write']}>
        <ItemSection data-slot="file-handling-settings-container">
          <ItemSectionHeader>
            <ItemSectionTitle>
              {m.settings_user_interface_file_handling_group()}
            </ItemSectionTitle>
          </ItemSectionHeader>

          <ItemGroup>
            <ProfileFileOpenerSelector />
          </ItemGroup>
        </ItemSection>
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
    <ItemSection data-slot="custom-css-presets-section">
      <ItemSectionHeader>
        <ItemSectionTitle>
          {m.settings_user_interface_custom_css_presets_group()}
        </ItemSectionTitle>
      </ItemSectionHeader>

      <ItemGroup>
        <SettingsLinkRow
          to="/main/settings/nyanpasu/interface/presets"
          title={m.settings_user_interface_custom_css_presets_group()}
          description={m.settings_user_interface_custom_css_presets_entry_count(
            { count: enabled },
          )}
          data-slot="custom-css-presets-entry"
        />
      </ItemGroup>
    </ItemSection>
  )
}

const CustomCssItemsEntry = () => {
  const customCss = useCustomCss()

  return (
    <ItemSection data-slot="custom-css-items-section">
      <ItemSectionHeader>
        <ItemSectionTitle>
          {m.settings_user_interface_custom_css_group()}
        </ItemSectionTitle>
      </ItemSectionHeader>

      <ItemGroup>
        <SettingsLinkRow
          to="/main/settings/nyanpasu/interface/custom-css"
          title={m.settings_user_interface_custom_css_group()}
          description={m.settings_user_interface_custom_css_entry_count({
            count: customCss.state.items.length,
          })}
          data-slot="custom-css-items-entry"
        />
      </ItemGroup>
    </ItemSection>
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
