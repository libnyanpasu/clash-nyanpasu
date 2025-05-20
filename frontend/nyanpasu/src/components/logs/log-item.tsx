import { useAsyncEffect } from 'ahooks'
import { CSSProperties, useState } from 'react'
import { formatAnsi } from '@/utils/shiki'
import { Box, SxProps, Theme, useColorScheme } from '@mui/material'
import { LogMessage } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import styles from './log-item.module.scss'

const colorMapping: { [key: string]: SxProps<Theme> } = {
  error: (theme) => ({
    color: theme.vars.palette.error.main,
  }),
  warning: (theme) => ({
    color: theme.vars.palette.warning.main,
  }),
  info: (theme) => ({
    color: theme.vars.palette.info.main,
  }),
}

export const LogItem = ({
  value,
  className,
}: {
  value: LogMessage
  className?: string
}) => {
  const [payload, setPayload] = useState(value.payload)

  const { mode } = useColorScheme()

  useAsyncEffect(async () => {
    setPayload(await formatAnsi(value.payload))
  }, [value.payload])

  return (
    <div
      className={cn('w-full p-4 pt-2 pb-0 font-mono select-text', className)}
    >
      <div className="flex gap-2">
        <span className="font-thin">{value.time}</span>

        <Box
          component="span"
          className="inline-block font-semibold uppercase"
          sx={colorMapping[value.type]}
        >
          {value.type}
        </Box>
      </div>

      <div className="pb-2 text-wrap">
        <div
          className={cn(styles.item, mode === 'dark' && styles.dark)}
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
