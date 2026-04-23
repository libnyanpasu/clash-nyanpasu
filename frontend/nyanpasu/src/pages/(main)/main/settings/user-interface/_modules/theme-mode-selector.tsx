import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import {
  ThemeMode,
  useExperimentalThemeContext,
} from '@/components/providers/theme-provider'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card'

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
    <SettingsCard data-slot="theme-mode-selector">
      <DropdownMenu align="end">
        <DropdownMenuTrigger asChild>
          <SettingsCardContent data-slot="theme-mode-selector-trigger" asChild>
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_user_interface_theme_mode_label()}
                  </ItemLabelText>

                  <ItemLabelDescription>
                    {themeMode ? messages[themeMode] : null}
                  </ItemLabelDescription>
                </ItemLabel>

                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </SettingsCardContent>
        </DropdownMenuTrigger>

        <DropdownMenuContent sideOffset={-16} alignOffset={16}>
          {Object.entries(messages).map(([key, message]) => (
            <DropdownMenuCheckboxItem
              checked={themeMode === key}
              key={key}
              onSelect={() => handleThemeModeChange(key)}
            >
              {message}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  )
}
