import { useLockFn } from 'ahooks'
import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { Typography } from '@mui/material'
import { useNyanpasu } from '@nyanpasu/interface'
import { BaseDialog, BaseDialogProps } from '@nyanpasu/ui'
import HotkeyInput from './hotkey-input'

export type HotkeyDialogProps = Omit<BaseDialogProps, 'title'>

const HOTKEY_FUNC = [
  'open_or_close_dashboard',
  'clash_mode_rule',
  'clash_mode_global',
  'clash_mode_direct',
  'clash_mode_script',
  'toggle_system_proxy',
  // "enable_system_proxy",
  // "disable_system_proxy",
  'toggle_tun_mode',
  // "enable_tun_mode",
  // "disable_tun_mode",
] as const

type AllowedHotkeyFunc = (typeof HOTKEY_FUNC)[number]

type Key = string

type HotKeyErrorMessages = {
  [K in AllowedHotkeyFunc]: string | null
}

type HotKeyLoading = {
  [K in AllowedHotkeyFunc]: boolean
}

type HotkeyMap = { [K in AllowedHotkeyFunc]: Key[] }

export default function HotkeyDialog({
  open,
  onClose,
  children,
  ...rest
}: HotkeyDialogProps) {
  const { t } = useTranslation()

  // 检查是否有快捷键重复
  const [duplicateItems, setDuplicateItems] = useState<string[]>([])
  const { nyanpasuConfig, setNyanpasuConfig } = useNyanpasu()

  const [hotkeyMap, setHotkeyMap] = useState<HotkeyMap>({} as HotkeyMap)

  useEffect(() => {
    if (open && Object.keys(hotkeyMap).length === 0) {
      const map = {} as typeof hotkeyMap
      nyanpasuConfig?.hotkeys?.forEach((text) => {
        const [func, key] = text.split(',').map((i) => i.trim())
        if (!func || !key) return
        map[func as AllowedHotkeyFunc] = key
          .split('+')
          .map((e) => e.trim())
          .map((k) => (k === 'PLUS' ? '+' : k))
      })
      setHotkeyMap(map)
      setDuplicateItems([])
    }
  }, [hotkeyMap, nyanpasuConfig?.hotkeys, open])

  const [errorMessages, setErrorMessages] = useState<HotKeyErrorMessages>(
    HOTKEY_FUNC.reduce(
      (acc, cur) => ({ ...acc, [cur]: null }),
      {} as HotKeyErrorMessages,
    ),
  )

  const [loading, setLoading] = useState<HotKeyLoading>(
    HOTKEY_FUNC.reduce(
      (acc, cur) => ({ ...acc, [cur]: false }),
      {} as HotKeyLoading,
    ),
  )

  const saveState = useLockFn(
    async (func: AllowedHotkeyFunc, hotkeyMap: HotkeyMap) => {
      const hotkeys = Object.entries(hotkeyMap)
        .map(([func, keys]) => {
          if (!func || !keys?.length) return ''

          const key = keys
            .map((k) => k.trim())
            .filter(Boolean)
            .map((k) => (k === '+' ? 'PLUS' : k))
            .join('+')

          if (!key) return ''
          return `${func},${key}`
        })
        .filter(Boolean)

      try {
        await setNyanpasuConfig({ hotkeys })
      } catch (err: unknown) {
        setErrorMessages((prev) => ({
          ...prev,
          [func]: formatError(err),
        }))
        await message(formatError(err), {
          kind: 'error',
        })
      }
    },
  )

  const onBlurCb = useCallback(
    (e: React.FocusEvent<HTMLInputElement>, func: string) => {
      const keys = Object.values(hotkeyMap).flat().filter(Boolean)
      const set = new Set(keys)
      if (keys.length !== set.size) {
        setDuplicateItems([...duplicateItems, func])
        return
      } else {
        setDuplicateItems(duplicateItems.filter((e) => e !== func))
      }

      setLoading((prev) => ({ ...prev, [func]: true }))

      saveState(func as AllowedHotkeyFunc, hotkeyMap)
        .catch(() => {
          setDuplicateItems([...duplicateItems, func])
        })
        .finally(() => {
          setLoading((prev) => ({ ...prev, [func]: false }))
        })
    },
    [duplicateItems, hotkeyMap, saveState],
  )

  return (
    <BaseDialog
      title={t('Hotkey Setting')}
      open={open}
      onClose={onClose}
      {...rest}
    >
      {children}
      <div className="grid-1 grid gap-3">
        {HOTKEY_FUNC.map((func) => (
          <div className="flex items-center justify-between px-2" key={func}>
            <Typography>{t(func)}</Typography>
            <HotkeyInput
              func={func}
              isDuplicate={
                duplicateItems.includes(func) || !!errorMessages[func]
              }
              onBlurCb={onBlurCb}
              loading={loading[func]}
              value={hotkeyMap[func] ?? []}
              onValueChange={(v) =>
                setHotkeyMap((prev) => ({ ...prev, [func]: v }))
              }
            />
          </div>
        ))}
      </div>
    </BaseDialog>
  )
}
