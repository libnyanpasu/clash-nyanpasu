import MaterialSymbolsArrowDownwardRounded from '~icons/material-symbols/arrow-downward-rounded'
import MaterialSymbolsArrowUpwardRounded from '~icons/material-symbols/arrow-upward-rounded'
import MaterialSymbolsMemoryRounded from '~icons/material-symbols/memory-rounded'
import MaterialSymbolsSettingsEthernetRounded from '~icons/material-symbols/settings-ethernet-rounded'
import { useAtomValue } from 'jotai'
import { useTranslation } from 'react-i18next'
import Dataline, { DatalineProps } from '@/components/dashboard/dataline'
import { atomIsDrawer } from '@/store'
import {
  MAX_CONNECTIONS_HISTORY,
  MAX_MEMORY_HISTORY,
  MAX_TRAFFIC_HISTORY,
  useClashConnections,
  useClashMemory,
  useClashTraffic,
  useSetting,
} from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'

export const DataPanel = () => {
  const { t } = useTranslation()

  const { data: clashTraffic } = useClashTraffic()

  const { data: clashMemory } = useClashMemory()

  const {
    query: { data: clashConnections },
  } = useClashConnections()

  const { value } = useSetting('clash_core')

  const isSupportMemory = value && !['mihomo', 'mihomo-alpha'].includes(value)

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
      icon: <MaterialSymbolsArrowDownwardRounded />,
      title: t('Download Traffic'),
      total: clashConnections?.at(-1)?.downloadTotal,
      type: 'speed',
    },
    {
      data: padData(
        clashTraffic?.map((item) => item.up),
        MAX_TRAFFIC_HISTORY,
      ),
      icon: <MaterialSymbolsArrowUpwardRounded />,
      title: t('Upload Traffic'),
      total: clashConnections?.at(-1)?.uploadTotal,
      type: 'speed',
    },
    {
      data: padData(
        clashConnections?.map((item) => item.connections?.length ?? 0),
        MAX_CONNECTIONS_HISTORY,
      ),
      icon: <MaterialSymbolsSettingsEthernetRounded />,
      title: t('Active Connections'),
      type: 'raw',
    },
  ]

  if (isSupportMemory) {
    Datalines.splice(2, 0, {
      data: padData(
        clashMemory?.map((item) => item.inuse),
        MAX_MEMORY_HISTORY,
      ),
      icon: <MaterialSymbolsMemoryRounded />,
      title: t('Memory'),
    })
  }

  const isDrawer = useAtomValue(atomIsDrawer)

  return Datalines.map((props, index) => {
    return (
      <Dataline
        {...props}
        className={cn(
          'max-h-1/8 min-h-40',
          // TODO: use tailwind container queries
          // Apply responsive grid classes directly
          'col-span-6 sm:col-span-12 md:col-span-6',
          isDrawer ? 'sm:col-span-6' : 'sm:col-span-12',
          isSupportMemory
            ? 'lg:col-span-3 xl:col-span-3'
            : 'lg:col-span-4 xl:col-span-4',
        )}
      />
    )
  })
}

export default DataPanel
