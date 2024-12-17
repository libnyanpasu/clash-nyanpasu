import { cloneElement, FC } from 'react'
import { useTranslation } from 'react-i18next'
import parseTraffic from '@/utils/parse-traffic'
import { type SvgIconComponent } from '@mui/icons-material'
import { Paper } from '@mui/material'
import { cn, Sparkline } from '@nyanpasu/ui'

export interface DatalineProps {
  className?: string
  data: number[]
  icon: SvgIconComponent
  title: string
  total?: number
  type?: 'speed' | 'raw'
}

export const Dataline: FC<DatalineProps> = ({
  data,
  icon,
  title,
  total,
  type,
  className,
}) => {
  const { t } = useTranslation()

  return (
    <Paper className={cn('relative !rounded-3xl', className)}>
      <Sparkline data={data} className="absolute rounded-3xl" />

      <div className="absolute top-0 flex h-full flex-col justify-between gap-4 p-4">
        <div className="flex items-center gap-2">
          {/* @ts-expect-error icon should be cloneable */}
          {cloneElement(icon)}

          <div className="font-bold">{title}</div>
        </div>

        <div className="text-shadow-md text-2xl font-bold">
          {type === 'raw' ? data.at(-1) : parseTraffic(data.at(-1)).join(' ')}
          {type === 'speed' && '/s'}
        </div>

        <div className="h-5">
          {total !== undefined && (
            <span className="text-shadow-sm">
              {t('Total')}: {parseTraffic(total).join(' ')}
            </span>
          )}
        </div>
      </div>
    </Paper>
  )
}

export default Dataline
