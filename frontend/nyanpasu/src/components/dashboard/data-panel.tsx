import { useInterval } from 'ahooks'
import { useAtomValue } from 'jotai'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import Dataline, { DatalineProps } from '@/components/dashboard/dataline'
import { atomIsDrawer } from '@/store'
import {
  ArrowDownward,
  ArrowUpward,
  MemoryOutlined,
  SettingsEthernet,
} from '@mui/icons-material'
import Grid from '@mui/material/Grid2'
import {
  ClashMemory,
  ClashTraffic,
  useClashConnections,
  useClashMemory,
  useClashTraffic,
  useSetting,
} from '@nyanpasu/interface'

export const DataPanel = () => {
  const { t } = useTranslation()

  const [traffic, setTraffice] = useState<ClashTraffic[]>(
    new Array(20).fill({ up: 0, down: 0 }),
  )

  const [memory, setMemory] = useState<ClashMemory[]>(
    new Array(20).fill({ inuse: 0, oslimit: 0 }),
  )

  const [connection, setConnection] = useState<
    {
      downloadTotal: number
      uploadTotal: number
      connections_length: number
    }[]
  >(
    new Array(20).fill({
      downloadTotal: 0,
      uploadTotal: 0,
      connections_length: 0,
    }),
  )

  const { data: clashTraffic } = useClashTraffic()

  const { data: clashMemory } = useClashMemory()

  const { data: clashConnections } = useClashConnections()

  useInterval(() => {
    setTraffice((prevData) => [
      ...prevData.slice(1),
      clashTraffic?.at(-1) ?? { up: 0, down: 0 },
    ])

    setMemory((prevData) => [
      ...prevData.slice(1),
      clashMemory?.at(-1) ?? { inuse: 0, oslimit: 0 },
    ])

    const connectionsData = clashConnections?.at(-1) ?? {
      downloadTotal: 0,
      uploadTotal: 0,
    }

    setConnection((prevData) => [
      ...prevData.slice(1),
      {
        downloadTotal: connectionsData.downloadTotal,
        uploadTotal: connectionsData.uploadTotal,
        connections_length: connectionsData.connections?.length ?? 0,
      },
    ])
  }, 1000)

  const { value } = useSetting('clash_core')

  const supportMemory = value && ['mihomo', 'mihomo-alpha'].includes(value)

  const Datalines: DatalineProps[] = [
    {
      data: traffic.map((item) => item.up),
      icon: ArrowUpward,
      title: t('Upload Traffic'),
      total: connection.at(-1)?.uploadTotal,
      type: 'speed',
    },
    {
      data: traffic.map((item) => item.down),
      icon: ArrowDownward,
      title: t('Download Traffic'),
      total: connection.at(-1)?.downloadTotal,
      type: 'speed',
    },
    {
      data: connection.map((item) => item.connections_length),
      icon: SettingsEthernet,
      title: t('Active Connections'),
      type: 'raw',
    },
  ]

  if (supportMemory) {
    Datalines.splice(2, 0, {
      data: memory.map((item) => item.inuse),
      icon: MemoryOutlined,
      title: t('Memory'),
    })
  }

  const isDrawer = useAtomValue(atomIsDrawer)

  const gridLayout = useMemo(
    () => ({
      sm: isDrawer ? 6 : 12,
      md: 6,
      lg: supportMemory ? 3 : 4,
      xl: supportMemory ? 3 : 4,
    }),
    [isDrawer, supportMemory],
  )

  return Datalines.map((props, index) => {
    return (
      <Grid key={`data-${index}`} size={gridLayout}>
        <Dataline {...props} className="max-h-1/8 min-h-40" />
      </Grid>
    )
  })
}

export default DataPanel
