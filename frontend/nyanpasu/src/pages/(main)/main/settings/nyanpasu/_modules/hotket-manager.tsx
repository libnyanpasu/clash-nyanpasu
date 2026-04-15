import CloseRounded from '~icons/material-symbols/close-rounded'
import EditOutlineRounded from '~icons/material-symbols/edit-outline-rounded'
import ErrorRounded from '~icons/material-symbols/error-rounded'
import { isEqual } from 'lodash-es'
import { useEffect, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Kbd } from '@/components/ui/kbd'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { parseHotkey } from '@/utils/parse-hotkey'
import { useHotkeyFunctions, useHotkeys } from '@nyanpasu/interface'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card'

type HotkeyMap = Record<string, string[]>

const HotkeyItem = ({
  keys,
  onChange,
}: {
  keys: string[]
  onChange: (keys: string[]) => Promise<void> | void
}) => {
  const [isListening, setIsListening] = useState(false)
  const [currentKeys, setCurrentKeys] = useState(keys)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    setCurrentKeys(keys)
  }, [keys])

  const handleSave = useLockFn(async (newKeys: string[]) => {
    if (isEqual(newKeys, keys)) {
      return true
    }

    try {
      if (newKeys.length > 0) {
        await onChange(newKeys)
      } else {
        await onChange([])
      }

      return true
    } catch (err) {
      // Revert to original keys on error
      setCurrentKeys(keys)

      message(formatError(err), {
        kind: 'error',
      })
      return false
    }
  })

  const handleStartListening = () => {
    setIsListening(true)
    setCurrentKeys([])
    inputRef.current?.focus()
  }

  const handleStopListening = useLockFn(async () => {
    setIsListening(false)

    if (currentKeys.length > 0) {
      await handleSave(currentKeys)
    }
  })

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    e.preventDefault()
    e.stopPropagation()

    const key = parseHotkey(e.key)
    if (key === 'UNIDENTIFIED') {
      return
    }

    setCurrentKeys((prev) => {
      const newKeys = [...prev, key]
      return [...new Set(newKeys)]
    })
  }

  const handleKeyUp = () => {
    handleStopListening()
  }

  const handleClear = (e: React.MouseEvent) => {
    e.stopPropagation()
    setCurrentKeys([])
    onChange([])
  }

  return (
    <div className="flex items-center gap-2">
      <div className="border-input relative flex flex-wrap items-center gap-1 px-1">
        {currentKeys.map((key) => (
          <Kbd key={key}>{key}</Kbd>
        ))}

        {currentKeys.length === 0 ? (
          isListening ? (
            <span className="text-on-surface animate-pulse text-xs">
              {m.settings_nyanpasu_hotkey_press_key()}
            </span>
          ) : (
            <span className="text-on-surface text-xs">
              {m.settings_nyanpasu_hotkey_no_key()}
            </span>
          )
        ) : null}
      </div>

      <Button
        icon
        onClick={isListening ? handleStopListening : handleStartListening}
        className="relative size-8 shrink-0"
        variant="raised"
      >
        {isListening ? <ErrorRounded /> : <EditOutlineRounded />}

        <input
          ref={inputRef}
          type="text"
          className="absolute inset-0 cursor-pointer opacity-0"
          onBlur={handleStopListening}
          onKeyDown={handleKeyDown}
          onKeyUp={handleKeyUp}
        />
      </Button>

      <Button
        icon
        onClick={handleClear}
        className="size-8 shrink-0"
        variant="raised"
      >
        <CloseRounded />
      </Button>
    </div>
  )
}

export default function HotkeyManager() {
  const [hotkeyMap, setHotkeyMap] = useState<HotkeyMap>({})

  const { data: supportedFuncs = [] } = useHotkeyFunctions()

  const { data: hotkeyStrings = [], mutate: patchHotkeys } = useHotkeys()

  // Parse hotkey strings into hotkeyMap when data changes
  useEffect(() => {
    const map: HotkeyMap = {}

    hotkeyStrings.forEach((text: string) => {
      const [func, key] = text.split(',').map((i: string) => i.trim())
      if (!func || !key) {
        return
      }

      map[func] = key.split('+').map((k: string) => (k === 'PLUS' ? '+' : k))
    })

    setHotkeyMap((prev) => (isEqual(prev, map) ? prev : map))
  }, [hotkeyStrings])

  const saveHotkeys = useLockFn(async (newMap: HotkeyMap) => {
    const hotkeys = Object.entries(newMap)
      .filter(([_, keys]) => keys && keys.length > 0)
      .map(([func, keys]) => {
        const key = keys.map((k) => (k === '+' ? 'PLUS' : k)).join('+')

        return `${func},${key}`
      })

    await patchHotkeys(hotkeys)
  })

  const handleChange = useLockFn(async (func: string, newKeys: string[]) => {
    const updated = {
      ...hotkeyMap,
      [func]: newKeys,
    }

    await saveHotkeys(updated)
    setHotkeyMap(updated)
  })

  const messages = {
    open_or_close_dashboard:
      m.settings_nyanpasu_hotkey_open_or_close_dashboard(),
    clash_mode_rule: m.settings_nyanpasu_hotkey_clash_mode_rule(),
    clash_mode_global: m.settings_nyanpasu_hotkey_clash_mode_global(),
    clash_mode_direct: m.settings_nyanpasu_hotkey_clash_mode_direct(),
    clash_mode_script: m.settings_nyanpasu_hotkey_clash_mode_script(),
    toggle_system_proxy: m.settings_nyanpasu_hotkey_toggle_system_proxy(),
    enable_system_proxy: m.settings_nyanpasu_hotkey_enable_system_proxy(),
    disable_system_proxy: m.settings_nyanpasu_hotkey_disable_system_proxy(),
    toggle_tun_mode: m.settings_nyanpasu_hotkey_toggle_tun_mode(),
    enable_tun_mode: m.settings_nyanpasu_hotkey_enable_tun_mode(),
    disable_tun_mode: m.settings_nyanpasu_hotkey_disable_tun_mode(),
  } satisfies Record<string, string>

  return (
    <SettingsCard data-slot="hotkey-manager">
      <SettingsCardContent
        className="gap-4 py-4"
        data-slot="hotkey-manager-content"
      >
        {supportedFuncs.map((func) => (
          <ItemContainer key={func}>
            <ItemLabel>
              <ItemLabelText>
                {messages[func as keyof typeof messages] ?? func}
              </ItemLabelText>
            </ItemLabel>

            <HotkeyItem
              keys={hotkeyMap[func] ?? []}
              onChange={(keys) => handleChange(func, keys)}
            />
          </ItemContainer>
        ))}
      </SettingsCardContent>
    </SettingsCard>
  )
}
