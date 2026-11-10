import { nanoid } from 'nanoid'
import { useCallback } from 'react'
import { CUSTOM_CSS_PRESETS } from '@/assets/custom-css-presets'
import { m } from '@/paraglide/messages'
import { compileCustomCss } from '@/utils/custom-css-compiler'
import {
  addItem,
  EMPTY_CUSTOM_CSS_STATE,
  removeItem,
  reorderItems,
  setPresetEnabled,
  updateItem,
  type CustomCssState,
} from '@/utils/custom-css-items'
import { message } from '@/utils/notification'
import { useKvStorage } from '@nyanpasu/query'

export const CUSTOM_CSS_KV_KEY = 'custom-css-items'

/** Where the single CSS string of earlier versions was kept. */
export const LEGACY_CUSTOM_CSS_KV_KEY = 'custom-css'
export const LEGACY_CUSTOM_CSS_COMPILED_KV_KEY = 'custom-css-compiled'

/** Tell the user that a custom CSS change was not saved. */
export function reportCustomCssError(error: unknown) {
  return message(m.custom_css_save_error(), { kind: 'error', error })
}

export default function useCustomCss() {
  // `null` until the first write: the legacy string has not been carried over.
  const [stored, setStored, { isLoading }] =
    useKvStorage<CustomCssState | null>(CUSTOM_CSS_KV_KEY, null)

  const state = stored ?? EMPTY_CUSTOM_CSS_STATE

  /** Apply `transition` to the saved state. Rejects when the write was not confirmed. */
  const change = useCallback(
    async (transition: (state: CustomCssState) => CustomCssState) => {
      const saved = await setStored((prev) =>
        transition(prev ?? EMPTY_CUSTOM_CSS_STATE),
      )

      if (!saved) {
        throw new Error('The custom CSS could not be saved')
      }
    },
    [setStored],
  )

  const add = useCallback(
    async (name: string, source = '') => {
      const id = nanoid()
      const compiled = source ? await compileCustomCss(source) : ''

      await change((current) =>
        addItem(current, { id, name, enabled: true, source, compiled }),
      )

      return id
    },
    [change],
  )

  /** Compile `source`, then save both. Rejects, saving nothing, if it does not compile. */
  const saveSource = useCallback(
    async (id: string, source: string) => {
      const compiled = source ? await compileCustomCss(source) : ''

      await change((current) => updateItem(current, id, { source, compiled }))
    },
    [change],
  )

  const rename = useCallback(
    (id: string, name: string) =>
      change((current) => updateItem(current, id, { name })),
    [change],
  )

  const setEnabled = useCallback(
    (id: string, enabled: boolean) =>
      change((current) => updateItem(current, id, { enabled })),
    [change],
  )

  const remove = useCallback(
    (id: string) => change((current) => removeItem(current, id)),
    [change],
  )

  const reorder = useCallback(
    (ids: string[]) => change((current) => reorderItems(current, ids)),
    [change],
  )

  const setPreset = useCallback(
    (id: string, enabled: boolean) =>
      change((current) => setPresetEnabled(current, id, enabled)),
    [change],
  )

  /** Copy a preset into an editable item, disabled so it does not apply twice. */
  const duplicatePreset = useCallback(
    async (presetId: string, name: string) => {
      const preset = CUSTOM_CSS_PRESETS.find((item) => item.id === presetId)

      if (!preset) return null

      const id = nanoid()
      const compiled = await compileCustomCss(preset.source)

      await change((current) =>
        addItem(current, {
          id,
          name,
          enabled: false,
          source: preset.source,
          compiled,
        }),
      )

      return id
    },
    [change],
  )

  return {
    state,
    /** Whether the legacy string has been carried over (or there was none to carry). */
    isMigrated: stored !== null,
    isLoading,
    setState: setStored,
    add,
    saveSource,
    rename,
    setEnabled,
    remove,
    reorder,
    setPreset,
    duplicatePreset,
  }
}
