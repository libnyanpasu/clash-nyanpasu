import { useMemoizedFn } from 'ahooks'
import { useEffect, useRef } from 'react'
import {
  CloseRounded,
  CropSquareRounded,
  FilterNoneRounded,
  HorizontalRuleRounded,
  PushPin,
  PushPinOutlined,
} from '@mui/icons-material'
import { alpha, Button, ButtonProps, useTheme } from '@mui/material'
import { saveWindowSizeState, useNyanpasu } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { useQueryClient, useSuspenseQuery } from '@tanstack/react-query'
import { listen, TauriEvent, UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { platform as getPlatform } from '@tauri-apps/plugin-os'

const appWindow = getCurrentWebviewWindow()

const CtrlButton = (props: ButtonProps) => {
  const { palette } = useTheme()

  return (
    <Button
      className="!size-8 !min-w-0"
      sx={{
        backgroundColor: alpha(palette.primary.main, 0.1),
        svg: { transform: 'scale(0.9)' },
      }}
      {...props}
    />
  )
}

export const LayoutControl = ({ className }: { className?: string }) => {
  const { nyanpasuConfig, setNyanpasuConfig } = useNyanpasu()
  const { data: isMaximized } = useSuspenseQuery({
    queryKey: ['isMaximized'],
    queryFn: () => appWindow.isMaximized(),
  })
  const queryClient = useQueryClient()
  const unlistenRef = useRef<UnlistenFn | null>(null)
  const platform = useRef(getPlatform())

  useEffect(() => {
    listen(TauriEvent.WINDOW_RESIZED, () => {
      queryClient.invalidateQueries({ queryKey: ['isMaximized'] })
    })
      .then((unlisten) => {
        unlistenRef.current = unlisten
      })
      .catch((error) => {
        console.error(error)
      })
  }, [queryClient])

  const toggleAlwaysOnTop = useMemoizedFn(async () => {
    const isAlwaysOnTop = !!nyanpasuConfig?.always_on_top
    await setNyanpasuConfig({ always_on_top: !isAlwaysOnTop })
    await appWindow.setAlwaysOnTop(!isAlwaysOnTop)
  })

  return (
    <div className={cn('flex gap-1', className)} data-tauri-drag-region>
      <CtrlButton onClick={toggleAlwaysOnTop}>
        {nyanpasuConfig?.always_on_top ? (
          <PushPin fontSize="small" style={{ transform: 'rotate(15deg)' }} />
        ) : (
          <PushPinOutlined
            fontSize="small"
            style={{ transform: 'rotate(15deg)' }}
          />
        )}
      </CtrlButton>

      <CtrlButton onClick={() => appWindow.minimize()}>
        <HorizontalRuleRounded fontSize="small" />
      </CtrlButton>

      <CtrlButton
        onClick={() => {
          appWindow.toggleMaximize().then((isMaximized) => {
            queryClient.invalidateQueries({ queryKey: ['isMaximized'] })
          })
        }}
      >
        {isMaximized ? (
          <FilterNoneRounded
            fontSize="small"
            style={{
              transform: 'rotate(180deg) scale(0.8)',
            }}
          />
        ) : (
          <CropSquareRounded fontSize="small" />
        )}
      </CtrlButton>

      <CtrlButton
        onClick={() => {
          if (platform.current === 'windows') {
            saveWindowSizeState().finally(() => {
              appWindow.close()
            })
          } else {
            appWindow.close()
          }
        }}
      >
        <CloseRounded fontSize="small" />
      </CtrlButton>
    </div>
  )
}
