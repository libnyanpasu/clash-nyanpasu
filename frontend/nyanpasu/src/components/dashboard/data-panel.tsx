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
  Connection,
  Memory,
  Traffic,
  useClashWS,
  useNyanpasu,
} from '@nyanpasu/interface'

export const DataPanel = () => {
  const { t } = useTranslation()

  const [traffic, setTraffice] = useState<Traffic[]>(
    new Array(20).fill({ up: 0, down: 0 }),
  )

  const [memory, setMemory] = useState<Memory[]>(
    new Array(20).fill({ inuse: 0 }),
  )

  const [connection, setConnection] = useState<
    {
      downloadTotal: number
      uploadTotal: number
      connections: number
    }[]
  >(
    new Array(20).fill({
      downloadTotal: 0,
      uploadTotal: 0,
      connections: 0,
    }),
  )

  const {
    traffic: { latestMessage: latestTraffic },
    memory: { latestMessage: latestMemory },
    connections: { latestMessage: latestConnections },
  } = useClashWS()

  useInterval(() => {
    const trafficData = latestTraffic?.data
      ? (JSON.parse(latestTraffic.data) as Traffic)
      : { up: 0, down: 0 }

    setTraffice((prevData) => [...prevData.slice(1), trafficData])

    const meomryData = latestMemory?.data
      ? (JSON.parse(latestMemory.data) as Memory)
      : { inuse: 0, oslimit: 0 }

    setMemory((prevData) => [...prevData.slice(1), meomryData])

    const connectionsData = latestConnections?.data
      ? (JSON.parse(latestConnections.data) as Connection.Response)
      : {
          downloadTotal: 0,
          uploadTotal: 0,
        }

    setConnection((prevData) => [
      ...prevData.slice(1),
      {
        downloadTotal: connectionsData.downloadTotal,
        uploadTotal: connectionsData.uploadTotal,
        connections: connectionsData.connections?.length ?? 0,
      },
    ])
  }, 1000)

  const { nyanpasuConfig } = useNyanpasu()

  const supportMemory =
    nyanpasuConfig?.clash_core &&
    ['mihomo', 'mihomo-alpha'].includes(nyanpasuConfig?.clash_core)

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
      data: connection.map((item) => item.connections),
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
