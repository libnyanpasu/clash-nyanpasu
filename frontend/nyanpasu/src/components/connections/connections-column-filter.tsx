/* eslint-disable camelcase */
import { useLockFn } from 'ahooks'
import { snakeCase } from 'change-case'
import dayjs from 'dayjs'
import { AnimatePresence, Reorder, useDragControls } from 'framer-motion'
import { useAtom } from 'jotai'
import { type MRT_ColumnDef } from 'material-react-table'
import { MouseEventHandler, useCallback, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { connectionTableColumnsAtom } from '@/store'
import parseTraffic from '@/utils/parse-traffic'
import { Cancel, Menu } from '@mui/icons-material'
import { Checkbox, CircularProgress, IconButton } from '@mui/material'
import { useClash } from '@nyanpasu/interface'
import { BaseDialog, BaseDialogProps } from '@nyanpasu/ui'
import { TableConnection } from './connections-table'

function CloseConnectionButton({ id }: { id: string }) {
  const { deleteConnections } = useClash()
  const closeConnect = useLockFn(async (id?: string) => {
    await deleteConnections(id)
  })
  const [loading, setLoading] = useState(false)

  const onClick: MouseEventHandler<HTMLButtonElement> = useCallback(
    (e) => {
      e.preventDefault()
      e.stopPropagation()
      setLoading(true)
      closeConnect(id).finally(() => setLoading(false))
    },
    [closeConnect, id],
  )

  return (
    <div className="flex w-full items-center justify-center gap-2">
      <IconButton
        color="primary"
        className="size-4"
        onClick={onClick}
        disabled={loading}
      >
        {loading ? <CircularProgress color="primary" /> : <Cancel />}
      </IconButton>
    </div>
  )
}

export const useColumns = (): Array<MRT_ColumnDef<TableConnection>> => {
  const { t } = useTranslation()

  return useMemo(
    () =>
      (
        [
          {
            header: 'Actions',
            size: 60,
            enableSorting: false,
            enableGlobalFilter: false,
            enableResizing: false,
            accessorFn: ({ id }) => <CloseConnectionButton id={id} />,
          },
          {
            header: 'Host',
            size: 240,
            accessorFn: ({ metadata }) =>
              metadata.host || metadata.destinationIP,
          },
          {
            header: 'Process',
            size: 140,
            accessorFn: ({ metadata }) => metadata.process,
          },
          {
            header: 'Downloaded',
            size: 88,
            accessorFn: ({ download }) => parseTraffic(download).join(' '),
            sortingFn: (rowA, rowB) =>
              rowA.original.download - rowB.original.download,
          },
          {
            header: 'Uploaded',
            size: 88,
            accessorFn: ({ upload }) => parseTraffic(upload).join(' '),
            sortingFn: (rowA, rowB) =>
              rowA.original.upload - rowB.original.upload,
          },
          {
            header: 'DL Speed',
            size: 88,
            accessorFn: ({ downloadSpeed }) =>
              parseTraffic(downloadSpeed).join(' ') + '/s',
            sortingFn: (rowA, rowB) =>
              (rowA.original.downloadSpeed || 0) -
              (rowB.original.downloadSpeed || 0),
          },
          {
            header: 'UL Speed',
            size: 88,
            accessorFn: ({ uploadSpeed }) =>
              parseTraffic(uploadSpeed).join(' ') + '/s',
            sortingFn: (rowA, rowB) =>
              (rowA.original.uploadSpeed || 0) -
              (rowB.original.uploadSpeed || 0),
          },
          {
            header: 'Chains',
            size: 360,
            accessorFn: ({ chains }) => [...chains].reverse().join(' / '),
          },
          {
            header: 'Rule',
            size: 200,
            accessorFn: ({ rule, rulePayload }) =>
              rulePayload ? `${rule} (${rulePayload})` : rule,
          },
          {
            header: 'Time',
            size: 120,
            accessorFn: ({ start }) => dayjs(start).fromNow(),
            sortingFn: (rowA, rowB) =>
              dayjs(rowA.original.start).diff(rowB.original.start),
          },
          {
            header: 'Source',
            size: 200,
            accessorFn: ({ metadata: { sourceIP, sourcePort } }) =>
              `${sourceIP}:${sourcePort}`,
          },
          {
            header: 'Destination IP',
            size: 200,
            accessorFn: ({ metadata: { destinationIP, destinationPort } }) =>
              `${destinationIP}:${destinationPort}`,
          },
          {
            header: 'Destination ASN',
            size: 200,
            accessorFn: ({ metadata: { destinationIPASN } }) =>
              `${destinationIPASN}`,
          },
          {
            header: 'Type',
            size: 160,
            accessorFn: ({ metadata }) =>
              `${metadata.type} (${metadata.network})`,
          },
        ] satisfies Array<MRT_ColumnDef<TableConnection>>
      ).map(
        (column) =>
          ({
            ...column,
            id: snakeCase(column.header),
            header: t(column.header),
          }) satisfies MRT_ColumnDef<TableConnection>,
      ),
    [t],
  )
}

export type ConnectionColumnFilterDialogProps = {} & Omit<
  BaseDialogProps,
  'title'
>

function ColItem({
  column,
  checked,
  onChange,
  value,
}: {
  column: MRT_ColumnDef<TableConnection>
  checked: boolean
  onChange: (e: React.ChangeEvent<HTMLInputElement>) => void
  value: [string, boolean]
}) {
  const controls = useDragControls()
  return (
    <Reorder.Item
      value={value}
      dragListener={false}
      dragControls={controls}
      className="flex gap-1"
    >
      <div className="flex-1">
        <Checkbox checked={checked} onChange={onChange} />
        {column.header}
      </div>
      <div className="w-12">
        <IconButton onPointerDown={(e) => controls.start(e)}>
          <Menu />
        </IconButton>
      </div>
    </Reorder.Item>
  )
}

export default function ConnectionColumnFilterDialog(
  props: ConnectionColumnFilterDialogProps,
) {
  const { t } = useTranslation()
  const columns = useColumns()
  const [filteredCols, setFilteredCols] = useAtom(connectionTableColumnsAtom)
  const sortedCols = useMemo(
    () =>
      columns
        .filter((o) => o.id !== 'actions')
        .sort((a, b) => {
          const aIndex = filteredCols.findIndex((o) => o[0] === a.id)
          const bIndex = filteredCols.findIndex((o) => o[0] === b.id)
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
    [columns, filteredCols],
  )

  const latestFilteredCols = sortedCols.map((column) => [
    column.id,
    filteredCols.find((o) => o[0] === column.id)?.[1] ?? true,
  ]) as Array<[string, boolean]>

  return (
    <BaseDialog title={t('Connection Columns')} {...props}>
      <div className="grid grid-cols-1 gap-1">
        <AnimatePresence>
          <Reorder.Group
            values={latestFilteredCols}
            onReorder={setFilteredCols}
          >
            {sortedCols.map((column, index) => (
              <ColItem
                key={column.id}
                column={column}
                checked={
                  filteredCols.find((o) => o[0] === column.id)?.[1] ?? true
                }
                onChange={(e) => {
                  console.log(e.target.checked)
                  const newCols = [...filteredCols]
                  newCols[index] = [newCols[index][0], e.target.checked]
                  console.log(newCols)
                  setFilteredCols(newCols)
                }}
                value={latestFilteredCols[index]}
              />
            ))}
          </Reorder.Group>
        </AnimatePresence>
      </div>
    </BaseDialog>
  )
}
