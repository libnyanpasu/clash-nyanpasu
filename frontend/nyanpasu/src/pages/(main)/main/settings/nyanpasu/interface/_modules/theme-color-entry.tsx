import { useExperimentalThemeContext } from '@/components/providers/theme-provider'
import { m } from '@/paraglide/messages'
import { SettingsLinkRow } from '../../../_modules/settings-link-row'
import { THEME_STYLE_MESSAGES } from './theme-style-messages'

export default function ThemeColorEntry() {
  const { themeOptions } = useExperimentalThemeContext()

  const style = THEME_STYLE_MESSAGES.find(
    (entry) => entry.variant === themeOptions.variant,
  )

  return (
    <SettingsLinkRow
      to="/main/settings/nyanpasu/interface/theme"
      leading={
        <span
          data-slot="theme-color-entry-swatch"
          className="border-outline-variant size-6 rounded-full border"
          style={{ backgroundColor: themeOptions.color }}
        />
      }
      title={m.settings_user_interface_theme_color_label()}
      description={m.settings_user_interface_theme_color_entry_description({
        color: themeOptions.color,
        style: style?.label() ?? themeOptions.variant,
      })}
      data-slot="theme-color-entry"
    />
  )
}
