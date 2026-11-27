import LanguageRounded from '~icons/material-symbols/language-rounded'
import PaletteRounded from '~icons/material-symbols/palette-rounded'
import TuneRounded from '~icons/material-symbols/tune-rounded'
import {
  MenubarContent,
  MenubarGroup,
  MenubarMenu,
  MenubarTrigger,
} from '@nyanpasu/ui/menubar'
import { CircularProgress } from '@nyanpasu/ui/progress'
import { useCapabilities } from '@/components/auth/capabilities'
import { useLanguage } from '@/components/providers/language-provider'
import { useExperimentalThemeContext } from '@/components/providers/theme-provider'
import {
  useSystemProxy,
  useTransparentProxy,
  useTunMode,
} from '@/hooks/use-proxy-settings'
import { m } from '@/paraglide/messages'
import { Locale, locales } from '@/paraglide/runtime'
import { ProxyMode, useProxyMode } from '@nyanpasu/query'
import { ThemeMode } from '@nyanpasu/theme'
import { useHeaderMenuPrimitives } from './header-menu-primitives'

const LanguageSelector = () => {
  const { mode, Sub, SubTrigger, SubContent, RadioGroup, RadioItem } =
    useHeaderMenuPrimitives()
  const { language, setLanguage } = useLanguage()

  const handleLanguageChange = (value: string) => {
    setLanguage(value as Locale)
  }

  return (
    <Sub>
      <SubTrigger>
        {mode === 'launcher' && (
          <LanguageRounded className="size-5 shrink-0" aria-hidden="true" />
        )}
        <span>{m.header_settings_action_language()}</span>
      </SubTrigger>

      <SubContent>
        <RadioGroup value={language} onValueChange={handleLanguageChange}>
          {Object.entries(locales).map(([key, value]) => (
            <RadioItem key={key} value={value}>
              {m.language(key, { locale: value })}
            </RadioItem>
          ))}
        </RadioGroup>
      </SubContent>
    </Sub>
  )
}

const ThemeModeSelector = () => {
  const { mode, Sub, SubTrigger, SubContent, RadioGroup, RadioItem } =
    useHeaderMenuPrimitives()
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
    <Sub>
      <SubTrigger>
        {mode === 'launcher' && (
          <PaletteRounded className="size-5 shrink-0" aria-hidden="true" />
        )}
        <span>{m.header_settings_action_theme_mode()}</span>
      </SubTrigger>

      <SubContent>
        <RadioGroup value={themeMode} onValueChange={handleThemeModeChange}>
          {Object.entries(messages).map(([key, value]) => (
            <RadioItem key={key} value={key}>
              {value}
            </RadioItem>
          ))}
        </RadioGroup>
      </SubContent>
    </Sub>
  )
}

const ProxySettings = () => {
  const {
    mode,
    Group,
    Sub,
    SubTrigger,
    SubContent,
    CheckboxItem,
    RadioGroup,
    RadioItem,
    Separator,
  } = useHeaderMenuPrimitives()

  const systemProxy = useSystemProxy()

  const tunMode = useTunMode()

  const transparentProxy = useTransparentProxy()

  const proxyMode = useProxyMode()

  // Switching the routing mode changes the core: `proxy` scope.
  const canProxy = useCapabilities().hasPermission('proxy:write')

  const proxyModeMessages = {
    global: m.settings_system_proxy_global_mode_label(),
    direct: m.settings_system_proxy_direct_mode_label(),
    rule: m.settings_system_proxy_rule_mode_label(),
  } satisfies Record<ProxyMode, string>

  const toggleItems = (
    <>
      <CheckboxItem
        key="system-proxy"
        className="group relative"
        checked={systemProxy.isActive}
        onCheckedChange={() => {
          systemProxy.execute()
        }}
        data-loading={String(systemProxy.isPending)}
      >
        <span>{m.settings_system_proxy_system_proxy_label()}</span>

        <CircularProgress
          className="invisible size-4 shrink-0 group-data-[loading=true]:visible"
          indeterminate
        />
      </CheckboxItem>

      <CheckboxItem
        key="tun-mode"
        className="group relative"
        checked={tunMode.isActive}
        disabled={tunMode.disabled || tunMode.isPending}
        title={
          tunMode.disabled
            ? m.error_transparent_proxy_tun_conflict()
            : undefined
        }
        onCheckedChange={() => {
          tunMode.execute()
        }}
        data-loading={String(tunMode.isPending)}
      >
        <span>{m.settings_system_proxy_tun_mode_label()}</span>

        <CircularProgress
          className="invisible size-4 shrink-0 group-data-[loading=true]:visible"
          indeterminate
        />
      </CheckboxItem>

      {transparentProxy.visible && (
        <CheckboxItem
          className="group relative"
          checked={transparentProxy.isActive}
          disabled={transparentProxy.disabled || transparentProxy.isPending}
          title={transparentProxy.reason}
          onCheckedChange={() => {
            transparentProxy.execute()
          }}
          data-loading={String(transparentProxy.isPending)}
        >
          <span>{m.settings_clash_transparent_proxy_label()}</span>
          <CircularProgress
            className="invisible size-4 shrink-0 group-data-[loading=true]:visible"
            indeterminate
          />
        </CheckboxItem>
      )}
    </>
  )

  return (
    <Sub>
      <SubTrigger>
        {mode === 'launcher' && (
          <TuneRounded className="size-5 shrink-0" aria-hidden="true" />
        )}
        <span>{m.header_settings_action_proxy_settings()}</span>
      </SubTrigger>

      <SubContent>
        {mode === 'launcher' ? (
          <Group>{toggleItems}</Group>
        ) : (
          <>
            {toggleItems}
            <Separator />
          </>
        )}

        <RadioGroup
          value={
            Object.entries(proxyMode.value).find(([, enabled]) => enabled)?.[0]
          }
          onValueChange={(value) => proxyMode.upsert(value as ProxyMode)}
        >
          {Object.keys(proxyMode.value).map((key) => (
            <RadioItem key={key} value={key} disabled={!canProxy}>
              {proxyModeMessages[key as ProxyMode]}
            </RadioItem>
          ))}
        </RadioGroup>
      </SubContent>
    </Sub>
  )
}

export function HeaderSettingsItems() {
  return (
    <>
      <LanguageSelector />

      <ThemeModeSelector />

      <ProxySettings />
    </>
  )
}

export default function HeaderSettingsAction() {
  return (
    <MenubarMenu value="settings">
      <MenubarTrigger className="min-h-8 py-1">
        {m.header_settings_action_title()}
      </MenubarTrigger>

      <MenubarContent>
        <MenubarGroup>
          <HeaderSettingsItems />
        </MenubarGroup>
      </MenubarContent>
    </MenubarMenu>
  )
}
