import { useAsyncEffect } from 'ahooks'
import { CSSProperties, useState } from 'react'
import { formatAnsi } from '@/utils/shiki'
import { useTheme } from '@mui/material'
import { LogMessage } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import styles from './log-item.module.scss'

export const LogItem = ({
  value,
  className,
}: {
  value: LogMessage
  className?: string
}) => {
  const { palette } = useTheme()

  const [payload, setPayload] = useState(value.payload)

  const colorMapping: { [key: string]: string } = {
    error: palette.error.main,
    warning: palette.warning.main,
    info: palette.info.main,
  }

  useAsyncEffect(async () => {
    setPayload(await formatAnsi(value.payload))
  }, [value.payload])

  return (
    <div
      className={cn('w-full p-4 pt-2 pb-0 font-mono select-text', className)}
    >
      <div className="flex gap-2">
        <span className="font-thin">{value.time}</span>

        <span
          className="inline-block font-semibold uppercase"
          style={{
            color: colorMapping[value.type],
          }}
        >
          {value.type}
        </span>
      </div>

      <div className="pb-2 text-wrap">
        <div
          className={cn(styles.item, palette.mode === 'dark' && styles.dark)}
          style={
            {
              '--item-font': 'var(--font-mono)',
            } as CSSProperties
          }
          dangerouslySetInnerHTML={{
            __html: payload,
          }}
        />
      </div>
    </div>
  )
}

export default LogItem
