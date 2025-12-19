import {
  ThemeMode,
  useExperimentalThemeContext,
} from '@/components/providers/theme-provider'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
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
        className="flex items-center justify-between px-3"
        data-slot="theme-mode-selection-card-content"
      >
        <div>{m.settings_user_interface_theme_mode_label()}</div>

        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="flat">{messages[themeMode]}</Button>
          </DropdownMenuTrigger>

          <DropdownMenuContent>
            <DropdownMenuRadioGroup
              value={themeMode}
              onValueChange={handleThemeModeChange}
            >
              {Object.values(ThemeMode).map((value) => (
                <DropdownMenuRadioItem key={value} value={value}>
                  {value}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      </SettingsCardContent>
    </SettingsCard>
  )
}
