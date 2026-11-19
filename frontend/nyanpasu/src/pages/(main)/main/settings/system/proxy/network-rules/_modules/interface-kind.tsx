import LanOutlineRounded from '~icons/material-symbols/lan-outline-rounded'
import NetworkCheckRounded from '~icons/material-symbols/network-check-rounded'
import SettingsEthernetRounded from '~icons/material-symbols/settings-ethernet-rounded'
import WifiRounded from '~icons/material-symbols/wifi-rounded'
import type { ComponentProps } from 'react'
import { m } from '@/paraglide/messages'
import type { InterfaceKind } from '@nyanpasu/rpc/types'

/** The connection types a rule can ask for; `any` is a rule that asks for none. */
export const INTERFACE_KIND_OPTIONS = [
  'any',
  'wifi',
  'ethernet',
  'other',
] as const satisfies readonly (InterfaceKind | 'any')[]

export type InterfaceKindOption = (typeof INTERFACE_KIND_OPTIONS)[number]

export const interfaceKindLabel = (kind: InterfaceKindOption) => {
  switch (kind) {
    case 'any':
      return m.settings_system_proxy_network_automation_interface_any()
    case 'wifi':
      return m.settings_system_proxy_network_automation_network_wifi_kind()
    case 'ethernet':
      return m.settings_system_proxy_network_automation_network_ethernet()
    case 'other':
      return m.settings_system_proxy_network_automation_network_other()
  }
}

export function InterfaceKindIcon({
  kind,
  ...props
}: ComponentProps<'svg'> & { kind: InterfaceKindOption }) {
  switch (kind) {
    case 'any':
      return <NetworkCheckRounded aria-hidden {...props} />
    case 'wifi':
      return <WifiRounded aria-hidden {...props} />
    case 'ethernet':
      return <SettingsEthernetRounded aria-hidden {...props} />
    case 'other':
      return <LanOutlineRounded aria-hidden {...props} />
  }
}
