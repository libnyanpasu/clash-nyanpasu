import {
  ItemGroup,
  ItemSection,
  ItemSectionHeader,
  ItemSectionTitle,
} from '@nyanpasu/ui/item-group'
import { RequireCapability } from '@/components/auth/capabilities'
import { ReadOnlyScope } from '@/components/auth/read-only-scope'
import { RestrictedContent } from '@/components/auth/restricted-content'
import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { PlatformLinux } from '@nyanpasu/platform/react'
import { createFileRoute } from '@tanstack/react-router'
import AllowLanSwitch from './_modules/allow-lan-switch'
import IPv6Switch from './_modules/ipv6-switch'
import MipsCongestionControllerSelector from './_modules/mips-congestion-controller-selector'
import {
  MixedPortConfig,
  OptionalPortConfig,
} from './_modules/proxy-port-config'
import TransparentProxyConfigSettings from './_modules/transparent-proxy-config'
import TunStackSelector from './_modules/tun-stack-selector'

export const Route = createFileRoute('/(main)/main/settings/clash/network/')({
  component: RouteComponent,
  staticData: { crumb: { title: () => m.settings_label_network() } },
})

const PortSettings = () => {
  return (
    <ItemSection data-slot="port-settings-container">
      <ItemSectionHeader>
        <ItemSectionTitle>
          {m.settings_clash_settings_port_label()}
        </ItemSectionTitle>
      </ItemSectionHeader>

      <ItemGroup>
        <MixedPortConfig />

        <OptionalPortConfig field="socks_port" />

        <OptionalPortConfig field="http_port" />
      </ItemGroup>
    </ItemSection>
  )
}

const LanSettings = () => {
  return (
    <ItemSection data-slot="lan-settings-container">
      <ItemSectionHeader>
        <ItemSectionTitle>
          {m.settings_clash_network_lan_label()}
        </ItemSectionTitle>
      </ItemSectionHeader>

      <ItemGroup>
        <AllowLanSwitch />

        <IPv6Switch />
      </ItemGroup>
    </ItemSection>
  )
}

const TunSettings = () => {
  return (
    <ItemSection data-slot="tun-settings-container">
      <ItemSectionHeader>
        <ItemSectionTitle>
          {m.settings_system_proxy_tun_mode_label()}
        </ItemSectionTitle>
      </ItemSectionHeader>

      <ItemGroup>
        <TunStackSelector />

        <MipsCongestionControllerSelector />
      </ItemGroup>
    </ItemSection>
  )
}

function RouteComponent() {
  return (
    <>
      <PageTitle />

      <RequireCapability
        permission="settings:read"
        fallback={<RestrictedContent />}
      >
        <ReadOnlyScope permissions={['settings:write', 'system:write']}>
          <div className="space-y-4 px-4 pb-4">
            <PortSettings />

            <PlatformLinux>
              <TransparentProxyConfigSettings />
            </PlatformLinux>

            <LanSettings />

            <TunSettings />
          </div>
        </ReadOnlyScope>
      </RequireCapability>
    </>
  )
}
