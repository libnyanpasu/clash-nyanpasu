import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import {
  SettingsTitle,
  SettingsTitlePlaceholder,
} from '../_modules/settings-title'
import AllowLanSwitch from './_modules/allow-lan-switch'
import IPv6Switch from './_modules/ipv6-switch'
import LogLevelSelector from './_modules/log-level-selector'
import TunStackSelector from './_modules/tun-stack-selector'

export const Route = createFileRoute(
  '/(experimental)/experimental/settings/clash-settings',
)({
  component: RouteComponent,
})

function RouteComponent() {
  return (
    <>
      <SettingsTitlePlaceholder />
      <SettingsTitle>{m.settings_system_proxy_title()}</SettingsTitle>

      <AllowLanSwitch />

      <IPv6Switch />

      <TunStackSelector />

      <LogLevelSelector />
    </>
  )
}
