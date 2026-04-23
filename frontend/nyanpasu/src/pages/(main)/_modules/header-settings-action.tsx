import { PropsWithChildren } from 'react'
import { useLanguage } from '@/components/providers/language-provider'
import {
  ThemeMode,
  useExperimentalThemeContext,
} from '@/components/providers/theme-provider'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { CircularProgress } from '@/components/ui/progress'
import { useSystemProxy, useTunMode } from '@/hooks/use-proxy-settings'
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

const ProxySettings = () => {
  const systemProxy = useSystemProxy()

  const tunMode = useTunMode()

  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger>
        {m.header_settings_action_proxy_settings()}
      </DropdownMenuSubTrigger>

      <DropdownMenuSubContent>
        <DropdownMenuCheckboxItem
          key="system-proxy"
          className="group relative"
          checked={systemProxy.isActive}
          onCheckedChange={() => {
            systemProxy.execute()
          }}
          data-loading={String(systemProxy.isPending)}
        >
          <span className="text-nowrap">
            {m.settings_system_proxy_system_proxy_label()}
          </span>

          <CircularProgress
            className="invisible size-4 group-data-[loading=true]:visible"
            indeterminate
          />
        </DropdownMenuCheckboxItem>

        <DropdownMenuCheckboxItem
          key="tun-mode"
          className="group relative"
          checked={tunMode.isActive}
          onCheckedChange={() => {
            tunMode.execute()
          }}
          data-loading={String(tunMode.isPending)}
        >
          <span className="text-nowrap">
            {m.settings_system_proxy_tun_mode_label()}
          </span>

          <CircularProgress
            className="invisible size-4 group-data-[loading=true]:visible"
            indeterminate
          />
        </DropdownMenuCheckboxItem>
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

        <ProxySettings />
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
