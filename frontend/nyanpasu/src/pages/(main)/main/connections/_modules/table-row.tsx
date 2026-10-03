import ChatInfoRounded from '~icons/material-symbols/chat-info-rounded'
import CloseRounded from '~icons/material-symbols/close-rounded'
import { sentenceCase } from 'change-case'
import { isValid, parseISO } from 'date-fns'
import { filesize } from 'filesize'
import { ComponentProps, memo } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { ContextMenuItem } from '@nyanpasu/ui/context-menu'
import { Modal, ModalClose, ModalContent, ModalTitle } from '@nyanpasu/ui/modal'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider'
import { m } from '@/paraglide/messages'
import { useLockFn } from '@nyanpasu/hooks'
import {
  type ClosedConnection,
  type Connection_Serialize,
  type ConnectionMetadataFields_Serialize,
} from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { RelativeTimeCell } from './cells'
import type { ConnectionRow } from './use-connection-rows'

// Keys added by ConnectionRow, plus the two wrapper fields, that should not
// be rendered as their own dialog row: `metadata` and `_extra` get their own
// sections below.
const INTERNAL_KEYS = new Set([
  'downloadSpeed',
  'uploadSpeed',
  'startMs',
  'metadata',
  '_extra',
])

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
    | Exclude<keyof Connection_Serialize, 'metadata' | '_extra'>
    | keyof ConnectionMetadataFields_Serialize,
    () => string
  >
>

// Fields only a closed record has
const CLOSED_FIELD_LABELS = {
  source: m.connections_column_source,
  closed: m.connections_column_closed_time,
}

// Fields the core adds later have no message yet, so show the key itself
function fieldLabel(key: string) {
  if (Object.hasOwn(FIELD_LABELS, key)) {
    return FIELD_LABELS[key as keyof typeof FIELD_LABELS]()
  }

  if (Object.hasOwn(CLOSED_FIELD_LABELS, key)) {
    return CLOSED_FIELD_LABELS[key as keyof typeof CLOSED_FIELD_LABELS]()
  }

  return sentenceCase(key)
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

  if (typeof value === 'string' && value.includes('T')) {
    const date = parseISO(value)
    if (isValid(date)) {
      return <RelativeTimeCell ms={date.getTime()} />
    }
  }

  // An unknown (`_extra`) field's value can itself be a nested JSON object.
  if (value !== null && typeof value === 'object') {
    return <span>{JSON.stringify(value)}</span>
  }

  return <span>{String(value)}</span>
}

// Memoized: a new sample re-renders only the fields whose values changed,
// while relative times follow the tick on their own.
const RowRender = memo(function RowRender({
  label,
  value,
}: {
  label: string
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  value: any
}) {
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
})

// A connection's fields as dialog rows, keyed for `RowRender`'s labels.
type DetailFields = Array<[key: string, value: unknown]>

export type ConnectionDetail = {
  fields: DetailFields
  metadata?: DetailFields
  // The connection is gone: a closed record, or a live connection's last
  // sample kept after it closed.
  closed: boolean
}

const isShown = (value: unknown) =>
  value !== undefined && value !== null && value !== ''

export function activeConnectionDetail(
  row: ConnectionRow,
  closed: boolean,
): ConnectionDetail {
  return {
    fields: [
      ...Object.entries(row).filter(
        ([key, value]) => !INTERNAL_KEYS.has(key) && isShown(value),
      ),
      ...Object.entries(row._extra).filter(
        ([, value]) => value !== undefined && value !== null,
      ),
    ],
    metadata: [
      ...Object.entries(row.metadata ?? {}).filter(
        ([key, value]) => key !== '_extra' && isShown(value),
      ),
      ...Object.entries(row.metadata?._extra ?? {}).filter(
        ([, value]) => value !== undefined && value !== null,
      ),
    ],
    closed,
  }
}

export function closedConnectionDetail(
  row: ClosedConnection,
): ConnectionDetail {
  const { dimensions } = row

  return {
    fields: (
      [
        ['id', row.id],
        ['host', dimensions.target],
        ['chains', dimensions.chains],
        ['rule', dimensions.rule.kind],
        ['rulePayload', dimensions.rule.payload],
        ['type', dimensions.protocol],
        ['source', dimensions.source],
        ['process', dimensions.process],
        ['upload', row.bytes.upload],
        ['download', row.bytes.download],
        // ISO strings, so `formatValue` shows them as times
        ['start', new Date(row.started_at).toISOString()],
        ['closed', new Date(row.closed_at).toISOString()],
      ] satisfies DetailFields
    ).filter(([, value]) => isShown(value)),
    closed: true,
  }
}

// One dialog for the whole table, selected by connection id: a dialog owned by
// a virtualized row would follow the row's position, and every row would build
// its hidden dialog content on each sample. Memoized, so a sample re-renders it
// only while it shows that connection.
export const ConnectionDetailModal = memo(function ConnectionDetailModal({
  detail,
  onClose,
  onCloseConnection,
}: {
  detail?: ConnectionDetail
  onClose: () => void
  onCloseConnection?: () => Promise<unknown>
}) {
  const handleCloseConnection = useLockFn(async () => {
    // frist close the dialog to avoid showing stale data when the deletion is slow
    onClose()

    await onCloseConnection?.()
  })

  return (
    <Modal
      open={detail !== undefined}
      onOpenChange={(open) => {
        if (!open) {
          onClose()
        }
      }}
    >
      <ModalContent>
        {detail && (
          <Card divider className="flex max-w-[80vw] min-w-96 flex-col">
            <CardHeader className="flex-row items-center gap-2">
              <ModalTitle>{m.connections_view_details()}</ModalTitle>

              {detail.closed && (
                <span className="bg-surface-variant text-on-surface-variant rounded-full px-2 py-0.5 text-xs">
                  {m.connections_tab_closed()}
                </span>
              )}
            </CardHeader>

            <CardContent asChild className="p-0">
              <ScrollArea className="max-h-[70vh] select-text">
                <div className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-2 p-4">
                  {detail.fields.map(([key, value]) => (
                    <RowRender key={key} label={key} value={value} />
                  ))}

                  {detail.metadata && (
                    <>
                      <h3 className="col-span-2 pt-4 pb-1 text-base font-semibold">
                        {m.connections_field_metadata()}
                      </h3>

                      {detail.metadata.map(([key, value]) => (
                        <RowRender key={key} label={key} value={value} />
                      ))}
                    </>
                  )}
                </div>
              </ScrollArea>
            </CardContent>

            <CardFooter className="gap-2">
              <ModalClose variant="flat">{m.common_close()}</ModalClose>

              {!detail.closed && onCloseConnection && (
                <Button onClick={handleCloseConnection}>
                  {m.connections_close_connection()}
                </Button>
              )}
            </CardFooter>
          </Card>
        )}
      </ModalContent>
    </Modal>
  )
})

export default function TableRow({
  onDoubleClick,
  onViewDetails,
  onCloseConnection,
  ...props
}: ComponentProps<'tr'> & {
  onViewDetails: () => void
  // Absent for a connection that is already closed
  onCloseConnection?: () => void
}) {
  return (
    <RegisterContextMenu>
      <RegisterContextMenuTrigger asChild>
        <tr
          onDoubleClick={(e) => {
            onDoubleClick?.(e)
            onViewDetails()
          }}
          {...props}
        />
      </RegisterContextMenuTrigger>

      <RegisterContextMenuContent>
        <ContextMenuItem onSelect={onViewDetails}>
          <ChatInfoRounded className="size-4" />
          <span>{m.connections_view_details()}</span>
        </ContextMenuItem>

        {onCloseConnection && (
          <ContextMenuItem onSelect={onCloseConnection}>
            <CloseRounded className="size-4" />
            <span>{m.connections_close_connection()}</span>
          </ContextMenuItem>
        )}
      </RegisterContextMenuContent>
    </RegisterContextMenu>
  )
}
