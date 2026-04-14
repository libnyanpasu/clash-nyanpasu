import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import DeviceResetRounded from '~icons/material-symbols/device-reset-rounded'
import { AnimatePresence, motion } from 'framer-motion'
import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { TrayImage } from '@/components/ui/image'
import { CircularProgress } from '@/components/ui/progress'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { message } from '@/utils/notification'
import { commands, unwrapResult } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { useQuery } from '@tanstack/react-query'
import { open } from '@tauri-apps/plugin-dialog'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

enum TrayIconMode {
  normal = 'normal',
  tun = 'tun',
  system_proxy = 'system_proxy',
}

const TrayIconItem = ({ mode }: { mode: TrayIconMode }) => {
  const [iconVersion, setIconVersion] = useState(0)

  const isIconSet = useQuery({
    queryKey: ['trayIcon', mode],
    queryFn: async () => {
      const path = await commands.isTrayIconSet(mode)

      const result = unwrapResult(path)

      return result !== null
    },
  })

  const [isLoading, setIsLoading] = useState(false)

  const handleChangeIcon = useLockFn(async () => {
    try {
      const selected = await open({
        directory: false,
        multiple: false,
        filters: [
          {
            name: 'Images',
            extensions: ['png', 'jpg', 'jpeg', 'bmp', 'ico'],
          },
        ],
      })

      if (Array.isArray(selected)) {
        throw new Error('Not Support')
      } else if (selected === null) {
        return null
      }

      setIsLoading(true)

      await commands.setTrayIcon(mode, selected)
      await isIconSet.refetch()
      setIconVersion((prev) => prev + 1)

      message(m.settings_nyanpasu_tray_icon_set_success(), {
        kind: 'info',
      })
    } catch (e) {
      console.error(e)
      message(m.settings_nyanpasu_tray_icon_set_failed(), {
        kind: 'error',
      })
    } finally {
      setIsLoading(false)
    }
  })

  const handleResetIcon = useLockFn(async () => {
    try {
      // null means reset
      await commands.setTrayIcon(mode, null)
      await isIconSet.refetch()
      setIconVersion((prev) => prev + 1)

      message(m.settings_nyanpasu_tray_icon_reset_success(), {
        kind: 'info',
      })
    } catch (e) {
      console.error(e)
      message(m.settings_nyanpasu_tray_icon_reset_failed(), {
        kind: 'error',
      })
    }
  })

  const messages = {
    [TrayIconMode.normal]: m.settings_nyanpasu_tray_icon_normal(),
    [TrayIconMode.tun]: m.settings_nyanpasu_tray_icon_tun(),
    [TrayIconMode.system_proxy]: m.settings_nyanpasu_tray_icon_system_proxy(),
  }

  return (
    <SettingsCard
      className="relative"
      data-mode={mode}
      data-is-set={isIconSet.data}
    >
      <AnimatePresence initial={false}>
        {isLoading && (
          <motion.div
            data-slot="core-manager-card-mask"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className={cn(
              'bg-primary/10 absolute inset-0 z-50 backdrop-blur-3xl',
              'flex items-center justify-center gap-4',
            )}
          >
            <CircularProgress className="size-8" indeterminate />

            <p className="text-sm">{m.settings_nyanpasu_tray_icon_loading()}</p>
          </motion.div>
        )}
      </AnimatePresence>

      <SettingsCardContent className="flex-row items-center" asChild>
        <Button
          className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base"
          onClick={handleChangeIcon}
        >
          <TrayImage className="size-12" mode={mode} version={iconVersion} />

          <div className="flex-1 text-base font-semibold">{messages[mode]}</div>

          <div className="flex items-center gap-2">
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="raised"
                  className="hover:bg-inverse-on-surface"
                  icon
                  onClick={(e) => {
                    e.stopPropagation()
                    handleResetIcon()
                  }}
                  asChild
                >
                  <span>
                    <DeviceResetRounded />
                  </span>
                </Button>
              </TooltipTrigger>

              <TooltipContent>
                {m.settings_nyanpasu_tray_icon_reset()}
              </TooltipContent>
            </Tooltip>

            <span className="text-sm">
              {m.settings_nyanpasu_tray_icon_edit()}
            </span>
            <ArrowForwardIosRounded />
          </div>
        </Button>
      </SettingsCardContent>
    </SettingsCard>
  )
}

export default function TrayIconConfig() {
  return Object.values(TrayIconMode).map((mode) => (
    <TrayIconItem key={mode} mode={mode} />
  ))
  // return (
  //   <SettingsCard>
  //     {/* <SettingsCardContent className="gap-4">
  //       <div className="flex items-center justify-between">
  //         <span>{m.settings_nyanpasu_tray_icon()}</span>
  //       </div>
  //     </SettingsCardContent> */}

  //     <SettingsCardContent className="grid grid-cols-1 gap-3 pb-5 sm:grid-cols-3">
  //       {Object.values(TrayIconMode).map((mode) => (
  //         <TrayIconItem key={mode} mode={mode} />
  //       ))}
  //     </SettingsCardContent>
  //   </SettingsCard>
  // )
}
