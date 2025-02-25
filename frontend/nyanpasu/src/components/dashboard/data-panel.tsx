import { useAtomValue } from 'jotai'
import { useMemo } from 'react'
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
  MAX_CONNECTIONS_HISTORY,
  MAX_MEMORY_HISTORY,
  MAX_TRAFFIC_HISTORY,
  useClashConnections,
  useClashMemory,
  useClashTraffic,
  useSetting,
} from '@nyanpasu/interface'

export const DataPanel = () => {
  const { t } = useTranslation()

  const { data: clashTraffic } = useClashTraffic()

  const { data: clashMemory } = useClashMemory()

  const {
    query: { data: clashConnections },
  } = useClashConnections()

  const { value } = useSetting('clash_core')

  const supportMemory = value && ['mihomo', 'mihomo-alpha'].includes(value)

  const padData = (data: (number | undefined)[] = [], max: number) =>
    Array(Math.max(0, max - data.length))
      .fill(0)
      .concat(data.slice(-max))

  const Datalines: DatalineProps[] = [
    {
      data: padData(
        clashTraffic?.map((item) => item.down),
        MAX_TRAFFIC_HISTORY,
      ),
      icon: ArrowDownward,
      title: t('Download Traffic'),
      total: clashConnections?.at(-1)?.downloadTotal,
      type: 'speed',
    },
    {
      data: padData(
        clashTraffic?.map((item) => item.up),
        MAX_TRAFFIC_HISTORY,
      ),
      icon: ArrowUpward,
      title: t('Upload Traffic'),
      total: clashConnections?.at(-1)?.uploadTotal,
      type: 'speed',
    },
    {
      data: padData(
        clashConnections?.map((item) => item.connections?.length),
        MAX_CONNECTIONS_HISTORY,
      ),
      icon: SettingsEthernet,
      title: t('Active Connections'),
      type: 'raw',
    },
  ]

  if (supportMemory) {
    Datalines.splice(2, 0, {
      data: padData(
        clashMemory?.map((item) => item.inuse),
        MAX_MEMORY_HISTORY,
      ),
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
