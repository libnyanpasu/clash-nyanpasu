import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import {
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../_modules/settings-card'
import { SettingsTitle } from '../_modules/settings-title'
import AllowLanSwitch from './_modules/allow-lan-switch'
import CoreManagerCard from './_modules/core-manager-card'
import FieldFilterCard from './_modules/field-filter-card'
import FieldFilterSwitch from './_modules/field-filter-switch'
import IPv6Switch from './_modules/ipv6-switch'
import LogLevelSelector from './_modules/log-level-selector'
import MixedPortConfig from './_modules/mixed-port-config'
import RandomPortSwitch from './_modules/random-port-switch'
import TunStackSelector from './_modules/tun-stack-selector'

export const Route = createFileRoute('/(main)/main/settings/clash')({
  component: RouteComponent,
})

const PatchSettings = () => {
  return (
    <div data-slot="patch-settings-container">
      <SettingsLabel>{m.settings_clash_settings_title()}</SettingsLabel>

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

        <TunStackSelector />

        <LogLevelSelector />
      </SettingsGroup>
    </div>
  )
}

const PortSettings = () => {
  return (
    <div data-slot="port-settings-container">
      <SettingsLabel>{m.settings_clash_settings_port_label()}</SettingsLabel>

      <SettingsGroup>
        <MixedPortConfig />

        <SettingsCard>
          <SettingsCardContent>
            <RandomPortSwitch />
          </SettingsCardContent>
        </SettingsCard>
      </SettingsGroup>
    </div>
  )
}

const CoreManagerSettings = () => {
  return (
    <div data-slot="core-manager-settings-container">
      <SettingsLabel>
        {m.settings_clash_core_manager_card_title()}
      </SettingsLabel>

      <SettingsGroup>
        <CoreManagerCard />
      </SettingsGroup>
    </div>
  )
}

const FieldFilterSettings = () => {
  return (
    <div data-slot="field-filter-settings-container">
      <SettingsLabel>
        {m.settings_clash_settings_field_filter_label()}
      </SettingsLabel>

      <div className="space-y-2">
        <SettingsCard>
          <SettingsCardContent>
            <FieldFilterSwitch />
          </SettingsCardContent>
        </SettingsCard>

        <FieldFilterCard />
      </div>
    </div>
  )
}

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_clash_settings_title()}</SettingsTitle>

      <div className="space-y-4 px-4 pb-4">
        <PatchSettings />

        <PortSettings />

        <CoreManagerSettings />

        <FieldFilterSettings />
      </div>
    </>
  )
}
