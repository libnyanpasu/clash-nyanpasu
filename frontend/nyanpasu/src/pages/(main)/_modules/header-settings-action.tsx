import {
  MenubarCheckboxItem,
  MenubarContent,
  MenubarMenu,
  MenubarRadioGroup,
  MenubarRadioItem,
  MenubarSeparator,
  MenubarSub,
  MenubarSubContent,
  MenubarSubTrigger,
  MenubarTrigger,
} from '@nyanpasu/ui/menubar'
import { CircularProgress } from '@nyanpasu/ui/progress'
import { useLanguage } from '@/components/providers/language-provider'
import { useExperimentalThemeContext } from '@/components/providers/theme-provider'
import { useSystemProxy, useTunMode } from '@/hooks/use-proxy-settings'
import { m } from '@/paraglide/messages'
import { Locale, locales } from '@/paraglide/runtime'
import { ProxyMode, useProxyMode } from '@nyanpasu/query'
import { ThemeMode } from '@nyanpasu/theme'

const LanguageSelector = () => {
  const { language, setLanguage } = useLanguage()

  const handleLanguageChange = (value: string) => {
    setLanguage(value as Locale)
  }

  return (
    <MenubarSub>
      <MenubarSubTrigger>
        {m.header_settings_action_language()}
      </MenubarSubTrigger>

      <MenubarSubContent>
        <MenubarRadioGroup
          value={language}
          onValueChange={handleLanguageChange}
        >
          {Object.entries(locales).map(([key, value]) => (
            <MenubarRadioItem key={key} value={value}>
              {m.language(key, { locale: value })}
            </MenubarRadioItem>
          ))}
        </MenubarRadioGroup>
      </MenubarSubContent>
    </MenubarSub>
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
    <MenubarSub>
      <MenubarSubTrigger>
        {m.header_settings_action_theme_mode()}
      </MenubarSubTrigger>

      <MenubarSubContent>
        <MenubarRadioGroup
          value={themeMode}
          onValueChange={handleThemeModeChange}
        >
          {Object.entries(messages).map(([key, value]) => (
            <MenubarRadioItem key={key} value={key}>
              {value}
            </MenubarRadioItem>
          ))}
        </MenubarRadioGroup>
      </MenubarSubContent>
    </MenubarSub>
  )
}

const ProxySettings = () => {
  const systemProxy = useSystemProxy()

  const tunMode = useTunMode()

  const proxyMode = useProxyMode()

  const proxyModeMessages = {
    global: m.settings_system_proxy_global_mode_label(),
    direct: m.settings_system_proxy_direct_mode_label(),
    rule: m.settings_system_proxy_rule_mode_label(),
    script: m.settings_system_proxy_script_mode_label(),
  } satisfies Record<ProxyMode, string>

  return (
    <MenubarSub>
      <MenubarSubTrigger>
        {m.header_settings_action_proxy_settings()}
      </MenubarSubTrigger>

      <MenubarSubContent>
        <MenubarCheckboxItem
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
        </MenubarCheckboxItem>

        <MenubarCheckboxItem
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
        </MenubarCheckboxItem>

        <MenubarSeparator />

        <MenubarRadioGroup
          value={
            Object.entries(proxyMode.value).find(([, enabled]) => enabled)?.[0]
          }
          onValueChange={(value) => proxyMode.upsert(value as ProxyMode)}
        >
          {Object.keys(proxyMode.value).map((key) => (
            <MenubarRadioItem key={key} value={key}>
              {proxyModeMessages[key as ProxyMode]}
            </MenubarRadioItem>
          ))}
        </MenubarRadioGroup>
      </MenubarSubContent>
    </MenubarSub>
  )
}

export default function HeaderSettingsAction() {
  return (
    <MenubarMenu value="settings">
      <MenubarTrigger className="min-h-8 py-1">
        {m.header_settings_action_title()}
      </MenubarTrigger>

      <MenubarContent>
        <LanguageSelector />

        <ThemeModeSelector />

        <ProxySettings />
      </MenubarContent>
    </MenubarMenu>
  )
}
