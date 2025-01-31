/* eslint-disable camelcase */
import { useAtomValue } from 'jotai'
import { cloneDeep } from 'lodash-es'
import { MaterialReactTable, useMaterialReactTable } from 'material-react-table'
import { MRT_Localization_EN } from 'material-react-table/locales/en'
import { MRT_Localization_RU } from 'material-react-table/locales/ru'
import { MRT_Localization_ZH_HANS } from 'material-react-table/locales/zh-Hans'
import { MRT_Localization_ZH_HANT } from 'material-react-table/locales/zh-Hant'
import { lazy, useDeferredValue, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { connectionTableColumnsAtom } from '@/store'
import { containsSearchTerm } from '@/utils'
import { Connection, useClashWS } from '@nyanpasu/interface'
import ContentDisplay from '../base/content-display'
import { useColumns } from './connections-column-filter'

const ConnectionDetailDialog = lazy(() => import('./connection-detail-dialog'))

export type TableConnection = Connection.Item & {
  downloadSpeed?: number
  uploadSpeed?: number
}

export interface TableMessage extends Omit<Connection.Response, 'connections'> {
  connections: TableConnection[]
}

export const ConnectionsTable = ({ searchTerm }: { searchTerm?: string }) => {
  const { t, i18n } = useTranslation()

  const {
    connections: { latestMessage },
  } = useClashWS()

  const historyMessage = useRef<TableMessage | null>(null)

  const connectionsMessage = useMemo(() => {
    if (!latestMessage?.data) return
    const result = JSON.parse(latestMessage.data) as Connection.Response
    const updatedConnections: TableConnection[] = []

    const filteredConnections = searchTerm
      ? result.connections?.filter((connection) =>
          containsSearchTerm(connection, searchTerm),
        )
      : result.connections

    filteredConnections?.forEach((connection) => {
      const previousConnection = historyMessage.current?.connections.find(
        (history) => history.id === connection.id,
      )

      const downloadSpeed = previousConnection
        ? connection.download - previousConnection.download
        : 0

      const uploadSpeed = previousConnection
        ? connection.upload - previousConnection.upload
        : 0

      updatedConnections.push({
        ...connection,
        downloadSpeed,
        uploadSpeed,
      })
    })

    const data = { ...result, connections: updatedConnections }

    historyMessage.current = data

    return data
  }, [latestMessage?.data, searchTerm])
  const deferredTableData = useDeferredValue(connectionsMessage?.connections)

  const locale = useMemo(() => {
    switch (i18n.language) {
      case 'zh-CN':
        return MRT_Localization_ZH_HANS
      case 'zh-TW':
        return MRT_Localization_ZH_HANT
      case 'ru':
        return MRT_Localization_RU
      case 'en':
      default:
        return MRT_Localization_EN
    }
  }, [i18n.language])

  const columns = useColumns()
  const tableColsOrder = useAtomValue(connectionTableColumnsAtom)
  const filteredColumns = useMemo(
    () =>
      columns
        .filter(
          (column) =>
            tableColsOrder.find((o) => o[0] === column.id)?.[1] ?? true,
        )
        .sort((a, b) => {
          const aIndex = tableColsOrder.findIndex((o) => o[0] === a.id)
          const bIndex = tableColsOrder.findIndex((o) => o[0] === b.id)
          if (aIndex === -1 && bIndex === -1) {
            return 0
          }
          if (aIndex === -1) {
            return 1
          }
          if (bIndex === -1) {
            return -1
          }
          return aIndex - bIndex
        }),
    [columns, tableColsOrder],
  )
  const columnOrder = useMemo(
    () => filteredColumns.map((column) => column.id) as string[],
    [filteredColumns],
  )

  const columnVisibility = useMemo(() => {
    return filteredColumns.reduce(
      (acc, column) => {
        acc[column.id as string] =
          tableColsOrder.find((o) => o[0] === column.id)?.[1] ?? true
        return acc
      },
      {} as Record<string, boolean>,
    )
  }, [filteredColumns, tableColsOrder])

  const [connectionDetailDialogOpen, setConnectionDetailDialogOpen] =
    useState(false)
  const [connectioNDetailDialogItem, setConnectionDetailDialogItem] = useState<
    Connection.Item | undefined
  >(undefined)

  const table = useMaterialReactTable({
    columns: filteredColumns,
    data: deferredTableData ?? [],
    initialState: {
      density: 'compact',
      columnPinning: {
        left: ['actions'],
      },
    },
    state: {
      columnOrder,
      columnVisibility,
    },
    defaultDisplayColumn: {
      enableResizing: true,
    },
    enableTopToolbar: false,
    enableColumnActions: false,
    enablePagination: false,
    enableBottomToolbar: false,
    enableColumnResizing: true,
    enableGlobalFilterModes: true,
    enableColumnPinning: true,
    muiTableContainerProps: {
      sx: { minHeight: '100%' },
      className: '!absolute !h-full !w-full',
    },
    muiTableBodyRowProps({ row }) {
      return {
        onClick() {
          const id = row.original.id
          const item = connectionsMessage?.connections.find((o) => o.id === id)
          if (item) {
            setConnectionDetailDialogItem(cloneDeep(item))
            setConnectionDetailDialogOpen(true)
          }
        },
      }
    },
    localization: locale,
    enableRowVirtualization: true,
    enableColumnVirtualization: true,
    rowVirtualizerOptions: { overscan: 5 },
    columnVirtualizerOptions: { overscan: 2 },
  })

  return connectionsMessage?.connections.length ? (
    <>
      <ConnectionDetailDialog
        item={connectioNDetailDialogItem}
        open={connectionDetailDialogOpen}
        onClose={() => setConnectionDetailDialogOpen(false)}
      />
      <MaterialReactTable table={table} />
    </>
  ) : (
    <ContentDisplay
      className="!absolute !h-full !w-full"
      message={t('No Connections')}
    />
  )
}

export default ConnectionsTable
