import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import CheckRounded from '~icons/material-symbols/check-rounded'
import DeviceResetRounded from '~icons/material-symbols/device-reset-rounded'
import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { useState, type ReactNode } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { MaterialShape } from '@nyanpasu/ui/material-shape'
import { CircularProgress } from '@nyanpasu/ui/progress'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { TrayImage } from '@/components/ui/image'
import { m } from '@/paraglide/messages'
import { mutations, queries } from '@/services/rpc'
import { pickFile, type SelectedFile } from '@/utils/file-picker'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { isWindows } from '@nyanpasu/platform'
import {
  invokeMutation,
  unwrapQueryOptions,
  useTrayIcon,
} from '@nyanpasu/query'
import { cn } from '@nyanpasu/utils'
import catNormal from '@root/backend/tauri/icons/tray/cat/normal.png'
import catSystemProxy from '@root/backend/tauri/icons/tray/cat/system-proxy.png'
import catTun from '@root/backend/tauri/icons/tray/cat/tun.png'
import mascotNormal from '@root/backend/tauri/icons/tray/mascot/normal.png'
import mascotSystemProxy from '@root/backend/tauri/icons/tray/mascot/system-proxy.png'
import mascotTun from '@root/backend/tauri/icons/tray/mascot/tun.png'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  SettingsCard,
  SettingsCardContent,
  SettingsLabel,
} from '../../_modules/settings-card'
import {
  TrayIconEditor,
  type TrayIconLibraryItem,
  type TrayIconStateMode,
} from './tray-icon-editor'

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

const bytesToBase64 = (bytes: Uint8Array) => {
  let binary = ''
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000))
  }

  return btoa(binary)
}

const imageBase64 = async (url: string) => {
  const response = await fetch(url)
  if (!response.ok)
    throw new Error(`Failed to load tray preset: ${response.status}`)

  return bytesToBase64(new Uint8Array(await response.arrayBuffer()))
}

const setTrayIconFromSelection = async (
  mode: TrayIconMode,
  selected: SelectedFile,
) => {
  if (selected.type === 'path') {
    await invokeMutation(mutations.setTrayIcon, [mode, selected.path])
  } else {
    const bytes = new Uint8Array(await selected.file.arrayBuffer())
    await invokeMutation(mutations.setTrayIconFromBytes, [
      mode,
      bytesToBase64(bytes),
    ])
  }
}

const IMAGE_FILTERS = [
  { name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'bmp', 'ico'] },
]

type TrayPreset = keyof typeof TRAY_PRESETS
type ActivePreset = TrayPreset | 'custom'

// The backend re-encodes saved icons, so presets are matched on decoded pixels
// rather than file bytes.
const imagePixels = async (src: string) => {
  const bitmap = await createImageBitmap(await (await fetch(src)).blob())
  const canvas = document.createElement('canvas')
  canvas.width = bitmap.width
  canvas.height = bitmap.height
  const context = canvas.getContext('2d')!
  context.drawImage(bitmap, 0, 0)
  bitmap.close()

  return context.getImageData(0, 0, canvas.width, canvas.height)
}

const isSamePixels = (a: ImageData, b: ImageData) =>
  a.width === b.width &&
  a.height === b.height &&
  a.data.every((value, index) => value === b.data[index])

const useActivePreset = () => {
  const normal = useTrayIcon(TrayIconMode.normal).data?.data_url
  const tun = useTrayIcon(TrayIconMode.tun).data?.data_url
  const systemProxy = useTrayIcon(TrayIconMode.system_proxy).data?.data_url
  const urls = [normal, tun, systemProxy]

  return useQuery({
    queryKey: ['trayActivePreset', ...urls],
    enabled: urls.every(Boolean),
    queryFn: async (): Promise<ActivePreset> => {
      const current = await Promise.all(urls.map((url) => imagePixels(url!)))

      for (const preset of Object.keys(TRAY_PRESETS) as TrayPreset[]) {
        const candidates = await Promise.all(
          Object.values(TrayIconMode).map((mode) =>
            imagePixels(TRAY_PRESETS[preset][mode]),
          ),
        )
        if (candidates.every((pixels, i) => isSamePixels(pixels, current[i]))) {
          return preset
        }
      }

      return 'custom'
    },
  })
}

const ModeCard = ({
  selected,
  disabled,
  loading,
  title,
  onSelect,
  children,
}: {
  selected: boolean
  disabled?: boolean
  loading?: boolean
  title: string
  onSelect: () => void
  children: ReactNode
}) => {
  const reducedMotion = useReducedMotion()

  return (
    <Button
      data-slot="tray-icon-mode"
      data-selected={selected}
      role="radio"
      aria-checked={selected}
      disabled={disabled}
      onClick={onSelect}
      className={cn(
        'relative isolate h-auto flex-col items-stretch gap-3 rounded-3xl p-4 text-left',
        // The sliding indicator paints the selected background.
        selected
          ? 'text-on-primary-container! bg-transparent!'
          : 'text-on-surface-variant! bg-surface-variant/40! hover:brightness-95',
      )}
    >
      {selected && (
        <motion.span
          layoutId="tray-icon-mode-indicator"
          data-slot="tray-icon-mode-indicator"
          transition={
            reducedMotion
              ? { duration: 0 }
              : { type: 'spring', stiffness: 520, damping: 40 }
          }
          className="bg-primary-container absolute inset-0 -z-10 rounded-3xl"
        />
      )}

      <span className="flex items-center justify-between gap-2">
        <span className="font-semibold">{title}</span>

        <span
          data-slot="tray-icon-mode-check"
          className={cn(
            'bg-primary text-on-primary flex size-5 items-center justify-center rounded-full',
            !selected && !loading && 'invisible',
          )}
        >
          {loading ? (
            <CircularProgress className="size-4" indeterminate />
          ) : (
            <CheckRounded className="size-4" />
          )}
        </span>
      </span>

      {children}
    </Button>
  )
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
      const selected = await pickFile(null, IMAGE_FILTERS)

      if (!selected) return null

      setIsLoading(true)

      await setTrayIconFromSelection(mode, selected)
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
          <MaterialShape className="bg-surface-variant size-12 shrink-0">
            <TrayImage className="size-8" mode={mode} version={iconVersion} />
          </MaterialShape>

          <div className="flex flex-1 flex-col">
            <span className="text-base font-semibold">{messages[mode]}</span>

            <span className="text-on-surface-variant text-xs">
              {isIconSet.data
                ? m.settings_nyanpasu_tray_icon_customized()
                : m.settings_nyanpasu_tray_icon_default()}
            </span>
          </div>

          <div className="flex items-center gap-2">
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  disabled={disabled || !isIconSet.data}
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

  const [isEditing, setIsEditing] = useState(false)
  const [applyingPreset, setApplyingPreset] = useState<TrayPreset>()
  const [pendingMode, setPendingMode] = useState<TrayIconStateMode>()
  const [revisions, setRevisions] = useState<Record<TrayIconStateMode, number>>(
    { normal: 0, tun: 0, system_proxy: 0 },
  )
  // The mode the user picked. Until then it follows the preset the saved icons
  // match, so a custom mix that happens to equal a preset is not mistaken for
  // that preset after the user chose to edit it.
  const [chosenMode, setChosenMode] = useState<ActivePreset>()

  const detectedPreset = useActivePreset().data
  const activeMode = chosenMode ?? detectedPreset
  const isCustom = activeMode === 'custom'
  const busy = isEditing || applyingPreset != null || pendingMode != null

  const lock = useLockFn(async (task: () => Promise<void>) => task())

  const refreshIcons = (modes: TrayIconMode[]) =>
    Promise.all(
      modes.flatMap((mode) => [
        queryClient.invalidateQueries({ queryKey: ['getTrayIcon', mode] }),
        queryClient.invalidateQueries({ queryKey: ['isTrayIconSet', mode] }),
      ]),
    )

  const applyPreset = (preset: TrayPreset) =>
    lock(async () => {
      setApplyingPreset(preset)
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
        setChosenMode(preset)
        message(m.settings_nyanpasu_tray_icon_set_success(), { kind: 'info' })
      } catch (error) {
        message(m.settings_nyanpasu_tray_icon_set_failed(), {
          kind: 'error',
          error,
        })
      } finally {
        // Refresh even after a failure: earlier writes may have succeeded.
        await refreshIcons(Object.values(TrayIconMode))
        setRevisions((prev) => ({
          normal: prev.normal + 1,
          tun: prev.tun + 1,
          system_proxy: prev.system_proxy + 1,
        }))
        setApplyingPreset(undefined)
      }
    })

  // Runs one state's write. `task` resolves to the success message, or to
  // null when the user cancelled and nothing was written.
  const updateStateIcon = (
    mode: TrayIconStateMode,
    task: () => Promise<string | null>,
    failure: string,
  ) =>
    lock(async () => {
      setPendingMode(mode)
      try {
        const success = await task()
        if (success == null) {
          return
        }

        setRevisions((prev) => ({ ...prev, [mode]: prev[mode] + 1 }))
        message(success, { kind: 'info' })
      } catch (error) {
        console.error(error)
        message(failure, { kind: 'error', error })
      } finally {
        await refreshIcons([mode as TrayIconMode])
        setPendingMode(undefined)
      }
    })

  const assignIcon = (mode: TrayIconStateMode, iconId: string) => {
    const item = library.find(({ id }) => id === iconId)
    if (!item) return

    return updateStateIcon(
      mode,
      async () => {
        await invokeMutation(mutations.setTrayIconFromBytes, [
          mode,
          await imageBase64(item.src),
        ])

        return m.settings_nyanpasu_tray_icon_set_success()
      },
      m.settings_nyanpasu_tray_icon_set_failed(),
    )
  }

  const uploadIcon = (mode: TrayIconStateMode) =>
    updateStateIcon(
      mode,
      async () => {
        const selected = await pickFile(null, IMAGE_FILTERS)
        if (!selected) return null

        await setTrayIconFromSelection(mode as TrayIconMode, selected)

        return m.settings_nyanpasu_tray_icon_set_success()
      },
      m.settings_nyanpasu_tray_icon_set_failed(),
    )

  const presetLabels = {
    cat: m.settings_nyanpasu_tray_icon_preset_cat(),
    mascot: m.settings_nyanpasu_tray_icon_preset_mascot(),
  }

  const modeLabels = {
    normal: m.settings_nyanpasu_tray_icon_normal(),
    tun: m.settings_nyanpasu_tray_icon_tun(),
    system_proxy: m.settings_nyanpasu_tray_icon_system_proxy(),
  }

  const library: TrayIconLibraryItem[] = (
    Object.keys(TRAY_PRESETS) as TrayPreset[]
  ).flatMap((preset) =>
    Object.values(TrayIconMode).map((mode) => ({
      id: `${preset}-${mode}`,
      src: TRAY_PRESETS[preset][mode],
      label: `${presetLabels[preset]} · ${modeLabels[mode]}`,
    })),
  )

  // Presets are Windows-only; other platforms keep the per-state editor.
  if (!isWindows) {
    return Object.values(TrayIconMode).map((mode) => (
      <TrayIconItem
        key={mode}
        mode={mode}
        disabled={busy}
        onBusyChange={setIsEditing}
      />
    ))
  }

  return (
    <>
      <div
        data-slot="tray-icon-config"
        aria-busy={busy}
        className="flex flex-col"
      >
        <SettingsLabel>
          {m.settings_nyanpasu_tray_icon_preset_title()}
        </SettingsLabel>

        <div
          role="radiogroup"
          aria-label={m.settings_nyanpasu_tray_icon()}
          className="grid grid-cols-1 gap-3 sm:grid-cols-3"
        >
          {(Object.keys(TRAY_PRESETS) as TrayPreset[]).map((preset) => (
            <ModeCard
              key={preset}
              title={presetLabels[preset]}
              selected={activeMode === preset}
              loading={applyingPreset === preset}
              disabled={busy}
              onSelect={() => applyPreset(preset)}
            >
              <span className="flex w-full justify-around gap-2">
                {Object.values(TrayIconMode).map((mode) => (
                  <MaterialShape key={mode} className="bg-surface size-12">
                    <img
                      src={TRAY_PRESETS[preset][mode]}
                      alt=""
                      className="size-7"
                    />
                  </MaterialShape>
                ))}
              </span>
            </ModeCard>
          ))}

          <ModeCard
            title={m.settings_nyanpasu_tray_icon_custom()}
            selected={isCustom}
            disabled={busy}
            onSelect={() => setChosenMode('custom')}
          >
            <span className="text-xs">
              {m.settings_nyanpasu_tray_icon_custom_description()}
            </span>
          </ModeCard>
        </div>
      </div>

      {activeMode && (
        <TrayIconEditor
          editable={isCustom}
          library={library}
          modeLabels={modeLabels}
          pendingMode={pendingMode}
          busy={busy}
          revisions={revisions}
          onAssign={assignIcon}
          onUpload={uploadIcon}
        />
      )}
    </>
  )
}
