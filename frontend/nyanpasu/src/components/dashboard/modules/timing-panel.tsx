import { useAtomValue } from 'jotai'
import { useTranslation } from 'react-i18next'
import { useColorForDelay } from '@/hooks/theme'
import { atomIsDrawer } from '@/store'
import { useSetting } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'

function LatencyTag({ name, value }: { name: string; value: number }) {
  const { t } = useTranslation()

  const color = useColorForDelay(value)

  return (
    <div className="flex justify-between gap-1">
      <div className="font-bold">{name}:</div>

      <div className="truncate" style={{ color }}>
        {value ? `${value.toFixed(0)} ms` : t('Timeout')}
      </div>
    </div>
  )
}

export const TimingPanel = ({ data }: { data: { [key: string]: number } }) => {
  const isDrawer = useAtomValue(atomIsDrawer)

  const { value } = useSetting('clash_core')

  const isSupportMemory = value && !['mihomo', 'mihomo-alpha'].includes(value)

  return (
    <div
      className={cn(
        'bg-surface dark:bg-on-surface-variant/30',
        'h-full rounded-3xl p-4',
        // TODO: use tailwind container queries
        // Apply responsive grid classes directly
        'col-span-6',
        isDrawer
          ? isSupportMemory
            ? 'sm:col-span-4'
            : 'sm:col-span-6'
          : isSupportMemory
            ? 'md:col-span-4 lg:col-span-3'
            : 'md:col-span-6 lg:col-span-4',
      )}
    >
      <div className="flex h-full flex-col justify-between">
        {Object.entries(data).map(([name, value]) => (
          <LatencyTag key={name} name={name} value={value} />
        ))}
      </div>
    </div>
  )
}

export default TimingPanel
