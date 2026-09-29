import ChatInfoRounded from '~icons/material-symbols/chat-info-rounded'
import CloseRounded from '~icons/material-symbols/close-rounded'
import { sentenceCase } from 'change-case'
import dayjs from 'dayjs'
import { filesize } from 'filesize'
import { ComponentProps } from 'react'
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import { ContextMenuItem } from '@/components/ui/context-menu'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
} from '@/components/ui/modal'
import { ScrollArea } from '@/components/ui/scroll-area'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import {
  useDeleteClashConnections,
  type ClashConnectionItem,
  type ClashConnectionMetadata,
} from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import { ConnectionRow } from '..'

// Keys added by ConnectionRow that should not be rendered in the dialog
const INTERNAL_KEYS = new Set(['closed', 'downloadSpeed', 'uploadSpeed'])

const FIELD_LABELS = {
  id: m.connections_field_id,
  upload: m.connections_field_upload,
  download: m.connections_field_download,
  start: m.connections_field_start,
  chains: m.connections_field_chains,
  rule: m.connections_field_rule,
  rulePayload: m.connections_field_rule_payload,
  network: m.connections_field_network,
  type: m.connections_field_type,
  host: m.connections_field_host,
  sourceIP: m.connections_field_source_ip,
  sourcePort: m.connections_field_source_port,
  destinationIP: m.connections_field_destination_ip,
  destinationPort: m.connections_field_destination_port,
  destinationIPASN: m.connections_field_destination_ip_asn,
  sourceGeoIP: m.connections_field_source_geo_ip,
  destinationGeoIP: m.connections_field_destination_geo_ip,
  process: m.connections_field_process,
  processPath: m.connections_field_process_path,
  dnsMode: m.connections_field_dns_mode,
  dscp: m.connections_field_dscp,
  inboundIP: m.connections_field_inbound_ip,
  inboundName: m.connections_field_inbound_name,
  inboundPort: m.connections_field_inbound_port,
  inboundUser: m.connections_field_inbound_user,
  remoteDestination: m.connections_field_remote_destination,
  sniffHost: m.connections_field_sniff_host,
  specialProxy: m.connections_field_special_proxy,
  specialRules: m.connections_field_special_rules,
} satisfies Partial<
  Record<
    | Exclude<keyof ClashConnectionItem, 'metadata'>
    | keyof ClashConnectionMetadata,
    () => string
  >
>

// Fields the core adds later have no message yet, so show the key itself
function fieldLabel(key: string) {
  return Object.hasOwn(FIELD_LABELS, key)
    ? FIELD_LABELS[key as keyof typeof FIELD_LABELS]()
    : sentenceCase(key)
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function formatValue(key: string, value: any): React.ReactNode {
  if (Array.isArray(value)) {
    return <span>{value.join(' / ')}</span>
  }

  const k = key.toLowerCase()

  if (k.includes('speed')) {
    return <span>{filesize(value, { standard: 'iec' })}/s</span>
  }

  if (k.includes('download') || k.includes('upload')) {
    return <span>{filesize(value, { standard: 'iec' })}</span>
  }

  if (k.includes('port') || k === 'id' || k.includes('ip')) {
    return <span>{value}</span>
  }

  const date = dayjs(value)

  if (date.isValid() && typeof value === 'string' && value.includes('T')) {
    return (
      <span title={date.format('YYYY-MM-DD HH:mm:ss')}>{date.fromNow()}</span>
    )
  }

  return <span>{String(value)}</span>
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function RowRender({ label, value }: { label: string; value: any }) {
  const key = label.toLowerCase()

  return (
    <>
      <div className="w-fit text-sm font-semibold">{fieldLabel(label)}</div>
      <div
        className={cn(
          'text-sm break-all',
          (key === 'id' ||
            key.includes('ip') ||
            key.includes('port') ||
            key.includes('destination') ||
            key.includes('path')) &&
            'font-mono',
        )}
      >
        {formatValue(key, value)}
      </div>
    </>
  )
}

// One dialog for the whole table, selected by connection id: a dialog owned by
// a virtualized row would follow the row's position, and every row would build
// its hidden dialog content on each sample.
export function ConnectionDetailModal({
  data,
  onClose,
}: {
  data?: ConnectionRow
  onClose: () => void
}) {
  const deleteConnections = useDeleteClashConnections()

  const handleCloseConnection = useLockFn(async () => {
    if (!data) {
      return
    }

    // frist close the dialog to avoid showing stale data when the deletion is slow
    onClose()

    await deleteConnections.mutateAsync(data.id)
  })

  return (
    <Modal
      open={data !== undefined}
      onOpenChange={(open) => {
        if (!open) {
          onClose()
        }
      }}
    >
      <ModalContent>
        {data && (
          <Card divider className="flex max-w-[80vw] min-w-96 flex-col">
            <CardHeader>
              <ModalTitle>{m.connections_view_details()}</ModalTitle>
            </CardHeader>

            <CardContent asChild className="p-0">
              <ScrollArea className="max-h-[70vh] select-text">
                <div className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-2 p-4">
                  {Object.entries(data)
                    .filter(
                      ([key, value]) =>
                        key !== 'metadata' &&
                        !INTERNAL_KEYS.has(key) &&
                        value !== undefined &&
                        value !== null &&
                        value !== '',
                    )
                    .map(([key, value]) => (
                      <RowRender key={key} label={key} value={value} />
                    ))}

                  <h3 className="col-span-2 pt-4 pb-1 text-base font-semibold">
                    {m.connections_field_metadata()}
                  </h3>

                  {Object.entries(data.metadata)
                    .filter(
                      ([, value]) =>
                        value !== undefined && value !== null && value !== '',
                    )
                    .map(([key, value]) => (
                      <RowRender key={key} label={key} value={value} />
                    ))}
                </div>
              </ScrollArea>
            </CardContent>

            <CardFooter className="gap-2">
              <ModalClose variant="flat">{m.common_close()}</ModalClose>

              <Button onClick={handleCloseConnection}>
                {m.connections_close_connection()}
              </Button>
            </CardFooter>
          </Card>
        )}
      </ModalContent>
    </Modal>
  )
}

export default function TableRow({
  data,
  onDoubleClick,
  onViewDetails,
  ...props
}: ComponentProps<'tr'> & {
  data: ConnectionRow
  onViewDetails: (id: string) => void
}) {
  const deleteConnections = useDeleteClashConnections()

  const handleCloseConnection = useLockFn(async () => {
    await deleteConnections.mutateAsync(data.id)
  })

  return (
    <RegisterContextMenu>
      <RegisterContextMenuTrigger asChild>
        <tr
          onDoubleClick={(e) => {
            onDoubleClick?.(e)
            onViewDetails(data.id)
          }}
          {...props}
        />
      </RegisterContextMenuTrigger>

      <RegisterContextMenuContent>
        <ContextMenuItem onSelect={() => onViewDetails(data.id)}>
          <ChatInfoRounded className="size-4" />
          <span>{m.connections_view_details()}</span>
        </ContextMenuItem>

        <ContextMenuItem onSelect={() => handleCloseConnection()}>
          <CloseRounded className="size-4" />
          <span>{m.connections_close_connection()}</span>
        </ContextMenuItem>
      </RegisterContextMenuContent>
    </RegisterContextMenu>
  )
}
