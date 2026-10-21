import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import {
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../_modules/settings-card'
import AllowLanSwitch from './_modules/allow-lan-switch'
import IPv6Switch from './_modules/ipv6-switch'
import {
  MixedPortConfig,
  OptionalPortConfig,
} from './_modules/proxy-port-config'
import TunStackSelector from './_modules/tun-stack-selector'

export const Route = createFileRoute('/(main)/main/settings/clash/network/')({
  component: RouteComponent,
})

const PortSettings = () => {
  return (
    <div data-slot="port-settings-container">
      <SettingsLabel>{m.settings_clash_settings_port_label()}</SettingsLabel>

      <SettingsGroup>
        <MixedPortConfig />

        <OptionalPortConfig field="socks_port" />

        <OptionalPortConfig field="http_port" />
      </SettingsGroup>
    </div>
  )
}

const LanSettings = () => {
  return (
    <div data-slot="lan-settings-container">
      <SettingsLabel>{m.settings_clash_network_lan_label()}</SettingsLabel>

      <SettingsGroup>
        <SettingsCard>
          <SettingsCardContent>
            <AllowLanSwitch />
          </SettingsCardContent>
        </SettingsCard>

        <SettingsCard>
          <SettingsCardContent>
            <IPv6Switch />
          </SettingsCardContent>
        </SettingsCard>
      </SettingsGroup>
    </div>
  )
}

const TunSettings = () => {
  return (
    <div data-slot="tun-settings-container">
      <SettingsLabel>{m.settings_system_proxy_tun_mode_label()}</SettingsLabel>

      <SettingsGroup>
        <TunStackSelector />
      </SettingsGroup>
    </div>
  )
}

function RouteComponent() {
  return (
    <>
      <PageTitle backTo="/main/settings">
        {m.settings_label_network()}
      </PageTitle>

      <div className="space-y-4 px-4 pb-4">
        <PortSettings />

        <LanSettings />

        <TunSettings />
      </div>
    </>
  )
}
