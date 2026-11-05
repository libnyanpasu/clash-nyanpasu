import { cva } from 'class-variance-authority'
import { m } from '@/paraglide/messages'
import type { StunFamilies, StunServerCapabilities } from '@nyanpasu/rpc/types'

/** The small tonal label of a server row, as transform rows show their type. */
export const stunChipVariants = cva([
  'inline-block rounded-md px-1.5 py-0.5 text-xs',
  'bg-secondary-container/60 text-on-secondary-container',
])

function familiesLabel(families: StunFamilies) {
  return [families.ipv4 && 'IPv4', families.ipv6 && 'IPv6']
    .filter(Boolean)
    .join(' / ')
}

/**
 * One chip per transport the server answers over, naming its families. The
 * chips are laid out by the caller's row.
 */
export default function StunCapabilities({
  capabilities,
}: {
  capabilities: StunServerCapabilities
}) {
  const transports = [
    ['UDP', capabilities.udp],
    ['TCP', capabilities.tcp],
  ] as const
  const supported = transports.filter(
    ([, families]) => families.ipv4 || families.ipv6,
  )

  if (supported.length === 0) {
    return (
      <span className="text-error text-xs" data-slot="stun-capabilities-none">
        {m.settings_public_ip_stun_no_transport()}
      </span>
    )
  }

  return supported.map(([transport, families]) => (
    <span
      key={transport}
      className={stunChipVariants()}
      data-slot="stun-capability"
    >
      {transport} {familiesLabel(families)}
    </span>
  ))
}
