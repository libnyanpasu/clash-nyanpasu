import {
  ThemeMode,
  useExperimentalThemeContext,
} from '@/components/providers/theme-provider'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { m } from '@/paraglide/messages'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

export default function ThemeModeSelector() {
  const { themeMode, setThemeMode } = useExperimentalThemeContext()

  const handleThemeModeChange = (value: string) => {
    setThemeMode(value as ThemeMode)
  }

  const messages = {
    [ThemeMode.LIGHT]: m.settings_user_interface_theme_mode_light(),
    [ThemeMode.DARK]: m.settings_user_interface_theme_mode_dark(),
    [ThemeMode.SYSTEM]: m.settings_user_interface_theme_mode_system(),
  } satisfies Record<ThemeMode, string>

  return (
    <SettingsCard data-slot="theme-mode-selection-card">
      <SettingsCardContent
        className="flex items-center justify-between px-2"
        data-slot="theme-mode-selection-card-content"
      >
        <Select
          variant="outlined"
          value={themeMode}
          onValueChange={handleThemeModeChange}
        >
          <SelectTrigger>
            <SelectValue
              placeholder={m.settings_user_interface_theme_mode_label()}
            >
              {themeMode ? messages[themeMode] : null}
            </SelectValue>
          </SelectTrigger>

          <SelectContent>
            {Object.entries(messages).map(([key, value]) => (
              <SelectItem key={key} value={key}>
                {value}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </SettingsCardContent>
    </SettingsCard>
  )
}
