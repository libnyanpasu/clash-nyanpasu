import { cloneElement, ComponentProps, ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import parseTraffic from '@/utils/parse-traffic'
import { cn, Sparkline } from '@nyanpasu/ui'

export interface DatalineProps extends ComponentProps<'div'> {
  data: number[]
  icon: ReactNode
  title: string
  total?: number
  type?: 'speed' | 'raw'
}

export const Dataline = ({
  data,
  icon,
  title,
  total,
  type,
  className,
  ...props
}: DatalineProps) => {
  const { t } = useTranslation()

  return (
    <div
      className={cn(
        'relative rounded-3xl',
        'bg-surface dark:bg-on-surface-variant/30',
        className,
      )}
      {...props}
    >
      <Sparkline data={data} className="absolute rounded-3xl" />

      <div className="absolute top-0 flex h-full flex-col justify-between gap-4 p-4">
        <div className="flex items-center gap-2">
          {/* @ts-expect-error icon should be cloneable */}
          {cloneElement(icon, { className: 'size-6' })}

          <div className="font-bold">{title}</div>
        </div>

        <div className="text-shadow-md text-2xl font-bold">
          {type === 'raw' ? data.at(-1) : parseTraffic(data.at(-1)).join(' ')}
          {type === 'speed' && '/s'}
        </div>

        <div className="h-5">
          {total === undefined ? undefined : (
            <span className="text-shadow-sm">
              {t('Total')}: {parseTraffic(total).join(' ')}
            </span>
          )}
        </div>
      </div>
    </div>
  )
}

export default Dataline
