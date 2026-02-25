import { PropsWithChildren } from 'react'
import { useLanguage } from '@/components/providers/language-provider'
import {
  ThemeMode,
  useExperimentalThemeContext,
} from '@/components/providers/theme-provider'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import { Locale, locales } from '@/paraglide/runtime'

const LanguageSelector = () => {
  const { language, setLanguage } = useLanguage()

  const handleLanguageChange = (value: string) => {
    setLanguage(value as Locale)
  }

  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger>
        {m.header_settings_action_language()}
      </DropdownMenuSubTrigger>

      <DropdownMenuSubContent>
        <DropdownMenuRadioGroup
          value={language}
          onValueChange={handleLanguageChange}
        >
          {Object.entries(locales).map(([key, value]) => (
            <DropdownMenuRadioItem key={key} value={value}>
              {m.language(key, { locale: value })}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuSubContent>
    </DropdownMenuSub>
  )
}

const ThemeModeSelector = () => {
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
    <DropdownMenuSub>
      <DropdownMenuSubTrigger>
        {m.header_settings_action_theme_mode()}
      </DropdownMenuSubTrigger>

      <DropdownMenuSubContent>
        <DropdownMenuRadioGroup
          value={themeMode}
          onValueChange={handleThemeModeChange}
        >
          {Object.entries(messages).map(([key, value]) => (
            <DropdownMenuRadioItem key={key} value={key}>
              {value}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuSubContent>
    </DropdownMenuSub>
  )
}

export default function HeaderSettingsAction({ children }: PropsWithChildren) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>{children}</DropdownMenuTrigger>

      <DropdownMenuContent>
        <LanguageSelector />

        <ThemeModeSelector />
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
