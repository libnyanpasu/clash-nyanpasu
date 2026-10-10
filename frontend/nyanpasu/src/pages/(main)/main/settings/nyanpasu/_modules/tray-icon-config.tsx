import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import DeviceResetRounded from '~icons/material-symbols/device-reset-rounded'
import { AnimatePresence, motion } from 'motion/react'
import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { CircularProgress } from '@nyanpasu/ui/progress'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { TrayImage } from '@/components/ui/image'
import { m } from '@/paraglide/messages'
import { mutations, queries } from '@/services/rpc'
import { pickFile } from '@/utils/file-picker'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { isWindows } from '@nyanpasu/platform'
import { invokeMutation, unwrapQueryOptions } from '@nyanpasu/query'
import { cn } from '@nyanpasu/utils'
import catNormal from '@root/backend/tauri/icons/tray/cat/normal.png'
import catSystemProxy from '@root/backend/tauri/icons/tray/cat/system-proxy.png'
import catTun from '@root/backend/tauri/icons/tray/cat/tun.png'
import mascotNormal from '@root/backend/tauri/icons/tray/mascot/normal.png'
import mascotSystemProxy from '@root/backend/tauri/icons/tray/mascot/system-proxy.png'
import mascotTun from '@root/backend/tauri/icons/tray/mascot/tun.png'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

enum TrayIconMode {
  normal = 'normal',
  tun = 'tun',
  system_proxy = 'system_proxy',
}

const TRAY_PRESETS = {
  cat: { normal: catNormal, tun: catTun, system_proxy: catSystemProxy },
  mascot: {
    normal: mascotNormal,
    tun: mascotTun,
    system_proxy: mascotSystemProxy,
  },
} as const

const imageBase64 = async (url: string) => {
  const response = await fetch(url)
  if (!response.ok)
    throw new Error(`Failed to load tray preset: ${response.status}`)
  const bytes = new Uint8Array(await response.arrayBuffer())
  let binary = ''
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000))
  }

  return btoa(binary)
}

const TrayIconItem = ({
  mode,
  disabled,
  onBusyChange,
}: {
  mode: TrayIconMode
  disabled: boolean
  onBusyChange: (busy: boolean) => void
}) => {
  const [iconVersion, setIconVersion] = useState(0)

  const isIconSetQuery = queries.isTrayIconSet(mode)
  const setTrayIcon = mutations.setTrayIcon
  const isIconSet = useQuery(
    unwrapQueryOptions(isIconSetQuery, isIconSetQuery.queryFn!),
  )

  const [isLoading, setIsLoading] = useState(false)

  const handleChangeIcon = useLockFn(async () => {
    onBusyChange(true)
    try {
      const selected = await pickFile(null, [
        {
          name: 'Images',
          extensions: ['png', 'jpg', 'jpeg', 'bmp', 'ico'],
        },
      ])

      if (!selected) return null

      setIsLoading(true)

      if (selected.type === 'path') {
        await invokeMutation(setTrayIcon, [mode, selected.path])
      } else {
        const bytes = new Uint8Array(await selected.file.arrayBuffer())
        let binary = ''
        for (let offset = 0; offset < bytes.length; offset += 0x8000) {
          binary += String.fromCharCode(
            ...bytes.subarray(offset, offset + 0x8000),
          )
        }
        await invokeMutation(mutations.setTrayIconFromBytes, [
          mode,
          btoa(binary),
        ])
      }
      await isIconSet.refetch()
      setIconVersion((prev) => prev + 1)

      message(m.settings_nyanpasu_tray_icon_set_success(), {
        kind: 'info',
      })
    } catch (e) {
      console.error(e)
      message(m.settings_nyanpasu_tray_icon_set_failed(), {
        kind: 'error',
        error: e,
      })
    } finally {
      setIsLoading(false)
      onBusyChange(false)
    }
  })

  const handleResetIcon = useLockFn(async () => {
    onBusyChange(true)
    try {
      // null means reset
      await invokeMutation(setTrayIcon, [mode, null])
      await isIconSet.refetch()
      setIconVersion((prev) => prev + 1)

      message(m.settings_nyanpasu_tray_icon_reset_success(), {
        kind: 'info',
      })
    } catch (e) {
      console.error(e)
      message(m.settings_nyanpasu_tray_icon_reset_failed(), {
        kind: 'error',
        error: e,
      })
    } finally {
      onBusyChange(false)
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
          disabled={disabled}
        >
          <TrayImage className="size-12" mode={mode} version={iconVersion} />

          <div className="flex-1 text-base font-semibold">{messages[mode]}</div>

          <div className="flex items-center gap-2">
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  disabled={disabled}
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
  const queryClient = useQueryClient()
  const [isApplying, setIsApplying] = useState(false)
  const [isEditing, setIsEditing] = useState(false)
  const [presetVersion, setPresetVersion] = useState(0)

  const applyPreset = useLockFn(async (preset: keyof typeof TRAY_PRESETS) => {
    setIsApplying(true)
    try {
      // Load all assets before changing any saved icon.
      const icons = await Promise.all(
        Object.values(TrayIconMode).map(async (mode) => ({
          mode,
          base64: await imageBase64(TRAY_PRESETS[preset][mode]),
        })),
      )
      for (const { mode, base64 } of icons) {
        await invokeMutation(mutations.setTrayIconFromBytes, [mode, base64])
      }
      message(m.settings_nyanpasu_tray_icon_set_success(), { kind: 'info' })
    } catch (error) {
      message(m.settings_nyanpasu_tray_icon_set_failed(), {
        kind: 'error',
        error,
      })
    } finally {
      // Refresh even after a failure: earlier writes may have succeeded.
      await Promise.all(
        Object.values(TrayIconMode).flatMap((mode) => [
          queryClient.invalidateQueries({ queryKey: ['getTrayIcon', mode] }),
          queryClient.invalidateQueries({ queryKey: ['isTrayIconSet', mode] }),
        ]),
      )
      setPresetVersion((version) => version + 1)
      setIsApplying(false)
    }
  })

  const presetLabels = {
    cat: m.settings_nyanpasu_tray_icon_preset_cat(),
    mascot: m.settings_nyanpasu_tray_icon_preset_mascot(),
  }
  const modeLabels = {
    normal: m.settings_nyanpasu_tray_icon_normal(),
    tun: m.settings_nyanpasu_tray_icon_tun(),
    system_proxy: m.settings_nyanpasu_tray_icon_system_proxy(),
  }

  return (
    <>
      {isWindows && (
        <SettingsCard data-slot="tray-icon-presets" aria-busy={isApplying}>
          <SettingsCardContent className="gap-3">
            <p className="text-sm">
              {m.settings_nyanpasu_tray_icon_preset_description()}
            </p>
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              {(['cat', 'mascot'] as const).map((preset) => (
                <Button
                  key={preset}
                  data-slot="tray-icon-preset"
                  variant="stroked"
                  className="h-auto flex-col gap-3 rounded-2xl p-4"
                  disabled={isApplying || isEditing}
                  onClick={() => applyPreset(preset)}
                >
                  <span className="font-semibold">{presetLabels[preset]}</span>
                  <span className="flex w-full justify-around gap-2">
                    {Object.values(TrayIconMode).map((mode) => (
                      <span
                        key={mode}
                        className="flex flex-col items-center gap-2"
                      >
                        <span className="bg-surface-variant flex size-10 items-center justify-center rounded-lg">
                          <img
                            src={TRAY_PRESETS[preset][mode]}
                            alt=""
                            className="size-6"
                          />
                        </span>
                        <span className="text-xs">{modeLabels[mode]}</span>
                      </span>
                    ))}
                  </span>
                  <span className="text-sm">
                    {isApplying
                      ? m.settings_nyanpasu_tray_icon_loading()
                      : m.settings_nyanpasu_tray_icon_preset_apply()}
                  </span>
                </Button>
              ))}
            </div>
          </SettingsCardContent>
        </SettingsCard>
      )}
      {Object.values(TrayIconMode).map((mode) => (
        <TrayIconItem
          key={`${mode}-${presetVersion}`}
          mode={mode}
          disabled={isApplying || isEditing}
          onBusyChange={setIsEditing}
        />
      ))}
    </>
  )
}
