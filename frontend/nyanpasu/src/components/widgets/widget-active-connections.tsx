import NetworkCheckRounded from '~icons/material-symbols/network-check-rounded'
import { filesize } from 'filesize'
import { memo, useDeferredValue, useMemo } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent } from '@nyanpasu/ui/card'
import { useDndGridContext } from '@nyanpasu/ui/dnd-grid'
import { m } from '@/paraglide/messages'
import { useClashConnectionDetails, useClashWSStatus } from '@nyanpasu/query'
import type { ClashConnection_Serialize } from '@nyanpasu/rpc/types'
import { Link } from '@tanstack/react-router'
import { WidgetComponentProps } from './consts'
import { useWidgetConfig } from './provider'
import { WidgetId } from './widget-config'
import WidgetItem from './widget-item'

function connectionTarget(connection: ClashConnection_Serialize) {
  return (
    connection.metadata?.host ||
    connection.metadata?.destinationIP ||
    connection.metadata?.remoteDestination ||
    null
  )
}

function processBasename(path: string | null | undefined) {
  if (!path) return null
  return path.split(/[\\/]/).at(-1) || path
}

function ActiveConnectionsPreview({ id, onCloseClick }: WidgetComponentProps) {
  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.ActiveConnections}
      minW={4}
      minH={3}
      onCloseClick={onCloseClick}
    >
      <Card className="size-full" data-slot="widget-active-connections-card">
        <CardContent className="flex size-full flex-col gap-3">
          <div className="flex items-center gap-2 font-bold">
            <NetworkCheckRounded className="size-5" />
            {m.dashboard_widget_active_connections_title()}
          </div>
          <p className="text-on-surface-variant text-sm">
            {m.dashboard_widget_active_connections_preview()}
          </p>
          <div className="flex-1" />
          <div className="text-on-surface-variant text-xs">
            {m.dashboard_widget_active_connections_preview_rows()}
          </div>
        </CardContent>
      </Card>
    </WidgetItem>
  )
}

const ActiveConnectionsLive = memo(function ActiveConnectionsLive({
  id,
  onCloseClick,
  canAct,
  expanded,
}: WidgetComponentProps & { canAct: boolean; expanded: boolean }) {
  const config = useWidgetConfig(id, WidgetId.ActiveConnections)
  const { data, status, connectorState, retry } = useClashConnectionDetails()
  const { isLoading: isClashStatusLoading } = useClashWSStatus()
  const details = useDeferredValue(data)

  const connections = useMemo(() => {
    const direction = config.sort
    return [...(details?.connections ?? [])]
      .sort((left, right) => {
        const leftRate =
          direction === 'upload'
            ? left.uploadSpeed
            : direction === 'total'
              ? left.downloadSpeed + left.uploadSpeed
              : left.downloadSpeed
        const rightRate =
          direction === 'upload'
            ? right.uploadSpeed
            : direction === 'total'
              ? right.downloadSpeed + right.uploadSpeed
              : right.downloadSpeed
        return rightRate - leftRate || left.id.localeCompare(right.id)
      })
      .slice(0, expanded ? config.topN : Math.min(config.topN, 3))
  }, [config.sort, config.topN, details, expanded])

  const retryDisabled =
    !canAct || connectorState !== 'connected' || status === 'connecting'

  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.ActiveConnections}
      minW={4}
      minH={3}
      onCloseClick={onCloseClick}
    >
      <Card className="size-full" data-slot="widget-active-connections-card">
        <CardContent className="flex size-full min-h-0 flex-col gap-2">
          <div className="flex items-center justify-between gap-2">
            <div className="flex min-w-0 items-center gap-2 font-bold">
              <NetworkCheckRounded className="size-5 shrink-0" />
              <span className="truncate">
                {m.dashboard_widget_active_connections_title()}
              </span>
            </div>
            <Link
              aria-disabled={!canAct}
              tabIndex={canAct ? 0 : -1}
              onClick={(event) => {
                if (!canAct) event.preventDefault()
              }}
              className={`text-primary shrink-0 text-xs hover:underline ${!canAct ? 'pointer-events-none opacity-50' : ''}`}
              to="/main/connections"
            >
              {m.dashboard_widget_active_connections_open()}
            </Link>
          </div>

          {connectorState !== 'connected' ? (
            <p className="text-on-surface-variant text-sm" role="status">
              {isClashStatusLoading || connectorState === 'connecting'
                ? m.dashboard_widget_active_connections_connecting()
                : m.dashboard_widget_active_connections_disconnected()}
            </p>
          ) : status === 'error' ? (
            <div className="flex min-h-0 flex-1 flex-col justify-center gap-2">
              <p className="text-error text-sm" role="status">
                {m.dashboard_widget_active_connections_stream_error()}
              </p>
              <Button
                className="h-8 self-start px-3"
                disabled={retryDisabled}
                onClick={retry}
              >
                {m.dashboard_widget_active_connections_retry()}
              </Button>
            </div>
          ) : !details ? (
            <p className="text-on-surface-variant text-sm" role="status">
              {m.dashboard_widget_active_connections_waiting()}
            </p>
          ) : connections.length === 0 ? (
            <p className="text-on-surface-variant text-sm" role="status">
              {m.dashboard_widget_active_connections_empty()}
            </p>
          ) : (
            <div
              className="min-h-0 flex-1 overflow-auto"
              data-slot="widget-active-connections-list"
            >
              <ul className="flex flex-col gap-1">
                {connections.map((connection) => {
                  const process = config.showProcess
                    ? processBasename(connection.metadata?.process)
                    : null
                  const target = config.hideTargets
                    ? null
                    : connectionTarget(connection)

                  return (
                    <li
                      className="hover:bg-surface-variant/40 flex min-w-0 items-center justify-between gap-3 rounded-xl px-2 py-1 text-xs"
                      key={connection.id}
                    >
                      <div className="min-w-0">
                        <div className="truncate font-medium">
                          {target ??
                            m.dashboard_widget_active_connections_hidden_target()}
                        </div>
                        {process && (
                          <div className="text-on-surface-variant truncate">
                            {process}
                          </div>
                        )}
                      </div>
                      <div className="shrink-0 text-right tabular-nums">
                        <div>
                          ↓{' '}
                          {filesize(connection.downloadSpeed, {
                            standard: 'iec',
                          })}
                          /s
                        </div>
                        <div className="text-on-surface-variant">
                          ↑{' '}
                          {filesize(connection.uploadSpeed, {
                            standard: 'iec',
                          })}
                          /s
                        </div>
                      </div>
                    </li>
                  )
                })}
              </ul>
            </div>
          )}
        </CardContent>
      </Card>
    </WidgetItem>
  )
})

export function ActiveConnectionsWidget(props: WidgetComponentProps) {
  const { disabled, displayItems, isOverlay, sourceOnly } = useDndGridContext()
  if (sourceOnly || isOverlay) return <ActiveConnectionsPreview {...props} />
  const item = displayItems.find((candidate) => candidate.id === props.id)
  const expanded = (item?.w ?? 4) >= 6 && (item?.h ?? 3) >= 4
  return (
    <ActiveConnectionsLive
      {...props}
      canAct={disabled && !isOverlay}
      expanded={expanded}
    />
  )
}
