import ChatInfoRounded from '~icons/material-symbols/chat-info-rounded'
import CloseRounded from '~icons/material-symbols/close-rounded'
import { sentenceCase } from 'change-case'
import dayjs from 'dayjs'
import { filesize } from 'filesize'
import { ComponentProps, useState } from 'react'
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
import { useClashConnections } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import { ConnectionRow } from '..'

// Keys added by ConnectionRow that should not be rendered in the dialog
const INTERNAL_KEYS = new Set(['closed', 'downloadSpeed', 'uploadSpeed'])

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function formatValue(key: string, value: any): React.ReactNode {
  if (Array.isArray(value)) {
    return <span>{value.join(' / ')}</span>
  }

  const k = key.toLowerCase()

  if (k.includes('speed')) {
    return <span>{filesize(value)}/s</span>
  }

  if (k.includes('download') || k.includes('upload')) {
    return <span>{filesize(value)}</span>
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
      <div className="w-fit text-sm font-semibold">{sentenceCase(label)}</div>
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

export default function TableRow({
  data,
  onDoubleClick,
  ...props
}: ComponentProps<'tr'> & {
  data: ConnectionRow
}) {
  const { deleteConnections } = useClashConnections()

  const [open, setOpen] = useState(false)

  const handleCloseConnection = useLockFn(async () => {
    // frist close the dialog to avoid showing stale data when the deletion is slow
    if (open) {
      setOpen(false)
    }

    await deleteConnections.mutateAsync(data.id)
  })

  return (
    <>
      <RegisterContextMenu>
        <RegisterContextMenuTrigger asChild>
          <tr
            onDoubleClick={(e) => {
              onDoubleClick?.(e)
              setOpen(true)
            }}
            {...props}
          />
        </RegisterContextMenuTrigger>

        <RegisterContextMenuContent>
          <ContextMenuItem onSelect={() => setOpen(true)}>
            <ChatInfoRounded className="size-4" />
            <span>{m.connections_view_details()}</span>
          </ContextMenuItem>

          <ContextMenuItem onSelect={() => handleCloseConnection()}>
            <CloseRounded className="size-4" />
            <span>{m.connections_close_connection()}</span>
          </ContextMenuItem>
        </RegisterContextMenuContent>
      </RegisterContextMenu>

      <Modal open={open} onOpenChange={setOpen}>
        <ModalContent>
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
                    Metadata
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
        </ModalContent>
      </Modal>
    </>
  )
}
