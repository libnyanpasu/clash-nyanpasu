import { parseHotkey } from '@/utils/parse-hotkey'
import { Dangerous, DeleteRounded } from '@mui/icons-material'
import { alpha, CircularProgress, IconButton, useTheme } from '@mui/material'
import type {} from '@mui/material/themeCssVarsAugmentation'
import { CSSProperties, useEffect, useRef, useState } from 'react'
import { cn, Kbd } from '@nyanpasu/ui'
import styles from './hotkey-input.module.scss'

export interface Props extends React.HTMLAttributes<HTMLInputElement> {
  isDuplicate?: boolean
  value?: string[]
  onValueChange?: (value: string[]) => void
  func: string
  onBlurCb?: (e: React.FocusEvent<HTMLInputElement>, func: string) => void
  loading?: boolean
}

export default function HotkeyInput({
  isDuplicate = false,
  value,
  func,
  onValueChange,
  onBlurCb,
  // native
  className,
  loading,
  ...rest
}: Props) {
  const theme = useTheme()

  const changeRef = useRef<string[]>([])
  const [keys, setKeys] = useState(value || [])
  const [isClearing, setIsClearing] = useState(false)

  useEffect(() => {
    if (isClearing) {
      onBlurCb?.({} as React.FocusEvent<HTMLInputElement>, func)
      setIsClearing(false)
    }
  }, [func, isClearing, onBlurCb])

  return (
    <div className="flex items-center gap-2">
      <div className={cn('relative min-h-[36px] w-[165px]', styles.wrapper)}>
        <input
          className={cn(
            'absolute top-0 left-0 z-[1] h-full w-full opacity-0',
            styles.input,
            className,
          )}
          onKeyUp={() => {
            const ret = changeRef.current.slice()
            if (ret.length) {
              onValueChange?.(ret)
              changeRef.current = []
            }
          }}
          onKeyDown={(e) => {
            const evt = e.nativeEvent
            e.preventDefault()
            e.stopPropagation()
            const key = parseHotkey(evt.key)
            if (key === 'UNIDENTIFIED') return

            changeRef.current = [...new Set([...changeRef.current, key])]
            setKeys(changeRef.current)
          }}
          onBlur={(e) => {
            onBlurCb?.(e, func)
          }}
          {...rest}
        />
        <div
          className={cn(
            'box-border flex h-full min-h-[36px] w-full flex-wrap items-center rounded border border-solid px-1 py-1 last:mr-0',
            styles.items,
          )}
          style={
            {
              '--border-color': isDuplicate
                ? theme.palette.error.main
                : alpha(theme.palette.text.secondary, 0.15),
              '--input-focus-border-color': alpha(
                theme.palette.primary.main,
                0.75,
              ),
              '--input-hover-border-color': `rgba(${theme.vars.palette.common.background} / 0.23)`,
            } as CSSProperties
          }
        >
          {keys.map((key) => (
            <Kbd className="scale-75" key={key}>
              {key}
            </Kbd>
          ))}
          {loading && (
            <CircularProgress className="absolute right-2" size={13} />
          )}
          {isDuplicate && (
            <Dangerous
              className="absolute right-2 text-base"
              sx={[
                (theme) => ({
                  color: theme.palette.error.main,
                }),
              ]}
            />
          )}
        </div>
      </div>

      <IconButton
        size="small"
        title="Delete"
        color="inherit"
        onClick={() => {
          onValueChange?.([])
          setKeys([])
          setIsClearing(true)
        }}
      >
        <DeleteRounded fontSize="inherit" />
      </IconButton>
    </div>
  )
}
