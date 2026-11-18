import { Checkbox } from '@nyanpasu/ui/checkbox'
import { m } from '@/paraglide/messages'
import type { LocalNetwork } from '@nyanpasu/rpc/types'
import {
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
} from '../../../../_modules/settings-card'

/** Whether a network in CIDR notation lies in a private range (RFC 1918). */
function isPrivateNetwork(network: string) {
  const [first, second] = network.split(/[./]/).map(Number)

  return (
    first === 10 ||
    (first === 172 && second >= 16 && second <= 31) ||
    (first === 192 && second === 168)
  )
}

/**
 * The networks a newly chosen LAN scope allows: the private ones this computer is attached to, or
 * every one when none is private.
 */
export function defaultLanAllow(networks: LocalNetwork[]) {
  const all = networks.map(({ network }) => network)
  const local = all.filter(isPrivateNetwork)

  return local.length > 0 ? local : all
}

/**
 * The networks a LAN listener accepts connections from: those this computer is attached to,
 * followed by allowed networks that are not connected now, so they can still be removed. The last
 * allowed network cannot be removed, because the LAN scope allows at least one.
 */
export default function LanAllowList({
  value,
  networks,
  failed,
  disabled,
  onChange,
}: {
  value: string[]
  networks?: LocalNetwork[]
  failed: boolean
  disabled: boolean
  onChange: (next: string[]) => void
}) {
  const connected = networks ?? []

  const rows = [
    ...connected.map(({ network, interface: name, address }) => ({
      network,
      detail: `${name} · ${address}`,
    })),
    ...value
      .filter(
        (allowed) => !connected.some(({ network }) => network === allowed),
      )
      .map((network) => ({
        network,
        detail: m.settings_remote_access_lan_allow_disconnected(),
      })),
  ]

  return (
    <div className="flex flex-col gap-3" data-slot="remote-access-lan-allow">
      <ItemLabel>
        <ItemLabelText>
          {m.settings_remote_access_lan_allow_title()}
        </ItemLabelText>

        <ItemLabelDescription>
          {m.settings_remote_access_lan_allow_description()}
        </ItemLabelDescription>
      </ItemLabel>

      {rows.map(({ network, detail }) => {
        const checked = value.includes(network)

        return (
          <label
            key={network}
            className="flex items-center gap-3 text-sm"
            data-slot="remote-access-lan-allow-row"
          >
            <Checkbox
              checked={checked}
              disabled={disabled || (checked && value.length === 1)}
              onCheckedChange={(next) =>
                onChange(
                  next === true
                    ? [...value, network]
                    : value.filter((allowed) => allowed !== network),
                )
              }
            />

            <span className="flex flex-col">
              <span className="select-text">{network}</span>

              <span className="text-on-surface-variant text-xs">{detail}</span>
            </span>
          </label>
        )
      })}

      {failed ? (
        <p role="alert" className="text-error text-sm">
          {m.settings_remote_access_lan_allow_error()}
        </p>
      ) : (
        networks?.length === 0 &&
        value.length === 0 && (
          <ItemLabelDescription>
            {m.settings_remote_access_lan_allow_empty()}
          </ItemLabelDescription>
        )
      )}
    </div>
  )
}
