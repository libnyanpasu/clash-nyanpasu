import { useMemo } from 'react'
import {
  Item,
  ItemContent,
  ItemTitle,
  ItemTrailing,
} from '@nyanpasu/ui/item-group'
import { RequireCapability } from '@/components/auth/capabilities'
import { m } from '@/paraglide/messages'
import { useClashConfig } from '@nyanpasu/query'
import ManagedGuardFieldSelector from '../../_modules/managed-guard-field-selector'
import LanAddressesDialog from './lan-addresses-dialog'

/**
 * Who owns `allow-lan`, and the addresses other devices can use. The live
 * config holds the effective value, which a profile decides while the field
 * follows the profile.
 */
export default function AllowLanSettings() {
  const { query } = useClashConfig()

  const value = useMemo(() => query.data?.['allow-lan'], [query.data])

  return (
    <>
      <ManagedGuardFieldSelector field="allow-lan" />

      <RequireCapability permission="remote:read">
        <Item data-slot="allow-lan-addresses-container">
          <ItemContent>
            <ItemTitle>
              {m.settings_clash_settings_allow_lan_addresses_button()}
            </ItemTitle>
          </ItemContent>

          <ItemTrailing>
            <LanAddressesDialog
              allowLan={Boolean(value)}
              liveConfig={query.data}
            />
          </ItemTrailing>
        </Item>
      </RequireCapability>
    </>
  )
}
