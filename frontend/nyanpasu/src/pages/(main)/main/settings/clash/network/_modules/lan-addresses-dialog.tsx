import ContentCopyRounded from '~icons/material-symbols/content-copy-rounded'
import LanRounded from '~icons/material-symbols/lan-rounded'
import { useMemo, useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
  useModal,
} from '@nyanpasu/ui/modal'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { writeClipboardText } from '@/utils/clipboard'
import {
  useClashInfo,
  useClashSettings,
  useLocalNetworks,
} from '@nyanpasu/query'
import type { ClashApiSettings } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import {
  buildLanAddressGroups,
  resolveLiveLanPorts,
  type LanAddressGroup,
  type LanAddressItem,
  type LanPortKind,
} from './lan-addresses'

const PORT_LABELS = {
  controller: m.settings_clash_settings_allow_lan_addresses_port_controller,
  mixed: m.settings_clash_settings_mixed_port_label,
  socks5: m.settings_clash_settings_socks_port_label,
  http: m.settings_clash_settings_http_port_label,
} satisfies Record<LanPortKind, () => string>

function AllowLanDisabledNotice({ allowLan }: { allowLan: boolean }) {
  if (allowLan) {
    return null
  }

  return (
    <p
      role="note"
      data-slot="allow-lan-addresses-disabled-notice"
      className="bg-secondary-container text-on-secondary-container rounded-medium px-4 py-3 text-sm"
    >
      {m.settings_clash_settings_allow_lan_addresses_disabled_notice()}
    </p>
  )
}

function ControllerLoopbackNotice({ address }: { address: string | null }) {
  if (!address) {
    return null
  }

  return (
    <p
      role="note"
      data-slot="allow-lan-addresses-controller-loopback-notice"
      className="bg-surface-container-high text-on-surface-variant rounded-medium px-4 py-3 text-sm"
    >
      {m.settings_clash_settings_allow_lan_addresses_controller_loopback_notice(
        { address },
      )}
    </p>
  )
}

function CopyErrorAlert({ error }: { error: string | null }) {
  if (!error) {
    return null
  }

  return (
    <p role="alert" className="text-error text-sm">
      {error}
    </p>
  )
}

function LanAddressRow({
  entry,
  onCopy,
}: {
  entry: LanAddressItem
  onCopy: (address: string) => void
}) {
  return (
    <li className="flex items-center gap-3" data-slot="allow-lan-address-item">
      <span className="flex min-w-0 flex-1 flex-col">
        <code className="text-sm break-all select-text">{entry.address}</code>

        <span className="text-on-surface-variant text-xs">
          {PORT_LABELS[entry.kind]()}
        </span>
      </span>

      <Button
        type="button"
        icon
        aria-label={`${m.common_copy()} ${entry.address}`}
        onClick={() => onCopy(entry.address)}
      >
        <ContentCopyRounded />
      </Button>
    </li>
  )
}

function LanAddressGroupCard({
  group,
  onCopy,
}: {
  group: LanAddressGroup
  onCopy: (address: string) => void
}) {
  return (
    <section
      className="bg-surface-container-high rounded-large flex flex-col gap-2 p-4"
      data-slot="allow-lan-addresses-group"
    >
      <div className="flex items-baseline justify-between gap-2">
        <h3 className="text-sm font-medium">{group.interfaceName}</h3>

        <code className="text-on-surface-variant text-xs select-text">
          {group.ip}
        </code>
      </div>

      <ul className="flex flex-col gap-1">
        {group.entries.map((entry) => (
          <LanAddressRow
            key={`${entry.kind}-${entry.address}`}
            entry={entry}
            onCopy={onCopy}
          />
        ))}
      </ul>
    </section>
  )
}

function LanAddressesBody({
  isError,
  isSuccess,
  groups,
  hasPorts,
  onCopy,
}: {
  isError: boolean
  isSuccess: boolean
  groups: readonly LanAddressGroup[]
  hasPorts: boolean
  onCopy: (address: string) => void
}) {
  if (isError) {
    return (
      <p role="alert" className="text-error text-sm">
        {m.settings_clash_settings_allow_lan_addresses_error()}
      </p>
    )
  }

  if (isSuccess && groups.length === 0) {
    return (
      <p
        data-slot="allow-lan-addresses-empty"
        className="text-on-surface-variant text-sm"
      >
        {m.settings_clash_settings_allow_lan_addresses_empty_networks()}
      </p>
    )
  }

  if (!hasPorts) {
    return (
      <p
        data-slot="allow-lan-addresses-empty-ports"
        className="text-on-surface-variant text-sm"
      >
        {m.settings_clash_settings_allow_lan_addresses_empty_ports()}
      </p>
    )
  }

  return (
    <div className="flex flex-col gap-4" data-slot="allow-lan-addresses-groups">
      {groups.map((group) => (
        <LanAddressGroupCard
          key={`${group.interfaceName}-${group.ip}`}
          group={group}
          onCopy={onCopy}
        />
      ))}
    </div>
  )
}

function LanAddressesModalContent({
  allowLan,
  liveConfig,
}: {
  allowLan: boolean
  liveConfig: ClashApiSettings | undefined
}) {
  const { open } = useModal()
  const [error, setError] = useState<string | null>(null)

  const networks = useLocalNetworks({ enabled: open })
  const needsControllerFallback =
    open && liveConfig?.['external-controller'] == null
  const clashSettings = useClashSettings({ enabled: needsControllerFallback })
  const clashInfo = useClashInfo({ enabled: needsControllerFallback })

  const resolved = useMemo(
    () =>
      resolveLiveLanPorts(liveConfig, {
        clashSettings: clashSettings.query.data,
        clashInfo: clashInfo.data,
      }),
    [liveConfig, clashSettings.query.data, clashInfo.data],
  )

  const groups = useMemo(
    () => buildLanAddressGroups(networks.data ?? [], resolved),
    [networks.data, resolved],
  )

  const handleCopy = (address: string) => {
    setError(null)
    writeClipboardText(address).catch((failure) =>
      setError(formatError(failure)),
    )
  }

  return (
    <ModalContent onOpenAutoFocus={() => setError(null)}>
      <Card
        className={cn(
          'flex max-h-[85vh] w-[32rem] max-w-[calc(100vw-2rem)] flex-col',
          'overflow-hidden',
        )}
        data-slot="allow-lan-addresses-dialog"
      >
        <CardHeader className="shrink-0">
          <ModalTitle>
            {m.settings_clash_settings_allow_lan_addresses_title()}
          </ModalTitle>
        </CardHeader>

        <CardContent className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto">
          <p className="text-on-surface-variant text-sm">
            {m.settings_clash_settings_allow_lan_addresses_description()}
          </p>

          <AllowLanDisabledNotice allowLan={allowLan} />

          <ControllerLoopbackNotice
            address={resolved.controllerLoopbackAddress}
          />

          <LanAddressesBody
            isError={networks.isError}
            isSuccess={networks.isSuccess}
            groups={groups}
            hasPorts={resolved.ports.length > 0}
            onCopy={handleCopy}
          />

          <CopyErrorAlert error={error} />
        </CardContent>

        <CardFooter className="shrink-0">
          <ModalClose>{m.common_close()}</ModalClose>
        </CardFooter>
      </Card>
    </ModalContent>
  )
}

export default function LanAddressesDialog({
  allowLan,
  liveConfig,
}: {
  allowLan: boolean
  liveConfig: ClashApiSettings | undefined
}) {
  return (
    <Modal>
      <ModalTrigger asChild>
        <Button
          type="button"
          variant="stroked"
          className="flex items-center gap-1.5 px-4"
          data-slot="allow-lan-addresses-button"
        >
          <LanRounded className="size-4" />
          <span>{m.settings_clash_settings_allow_lan_addresses_button()}</span>
        </Button>
      </ModalTrigger>

      <LanAddressesModalContent allowLan={allowLan} liveConfig={liveConfig} />
    </Modal>
  )
}
