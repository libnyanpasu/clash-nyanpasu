/**
 * Pure state transitions for the custom CSS items.
 *
 * The whole state lives under one KV key, so a toggle, a rename and a reorder
 * are each a single atomic write. Bundled presets are never stored; only the
 * ids of the enabled ones are.
 */

export interface CustomCssItem {
  id: string
  name: string
  enabled: boolean
  /** The LESS/CSS the user wrote. */
  source: string
  /** `source` compiled to standard CSS, ready to inject. */
  compiled: string
}

export interface CustomCssState {
  items: CustomCssItem[]
  enabledPresets: string[]
}

/** A stylesheet shipped with the application. */
export interface CustomCssPreset {
  id: string
  name: () => string
  description: () => string
  /** Only offered on this platform; omitted for every platform. */
  platform?: 'macos'
  /** LESS/CSS source; compiled on demand and never stored. */
  source: string
}

export interface ActiveStyle {
  /** The id of the `<style>` element that carries `css`. */
  id: string
  css: string
}

export const EMPTY_CUSTOM_CSS_STATE: CustomCssState = {
  items: [],
  enabledPresets: [],
}

/** Id of the item created from the single CSS string of earlier versions. */
export const LEGACY_CUSTOM_CSS_ITEM_ID = 'legacy'

const STYLE_ID_PREFIX = 'nyanpasu-custom-css'

export function customCssStyleId(kind: 'preset' | 'item', id: string) {
  return `${STYLE_ID_PREFIX}-${kind}-${id}`
}

/** Whether `id` names one of the style elements created for custom CSS. */
export function isCustomCssStyleId(id: string) {
  return id.startsWith(`${STYLE_ID_PREFIX}-`)
}

/**
 * Carry the single CSS string of earlier versions over as one enabled item.
 * Returns the empty state when nothing was written.
 */
export function migrateLegacyCustomCss(
  source: string | null,
  compiled: string | null,
  name: string,
): CustomCssState {
  if (!source?.trim()) return EMPTY_CUSTOM_CSS_STATE

  return {
    items: [
      {
        id: LEGACY_CUSTOM_CSS_ITEM_ID,
        name,
        enabled: true,
        source,
        compiled: compiled ?? '',
      },
    ],
    enabledPresets: [],
  }
}

export function addItem(
  state: CustomCssState,
  item: CustomCssItem,
): CustomCssState {
  return { ...state, items: [...state.items, item] }
}

export function removeItem(state: CustomCssState, id: string): CustomCssState {
  return { ...state, items: state.items.filter((item) => item.id !== id) }
}

export function updateItem(
  state: CustomCssState,
  id: string,
  patch: Partial<Omit<CustomCssItem, 'id'>>,
): CustomCssState {
  return {
    ...state,
    items: state.items.map((item) =>
      item.id === id ? { ...item, ...patch } : item,
    ),
  }
}

/**
 * Put the items in the order of `ids`. An item missing from `ids` (added by
 * another window meanwhile) keeps its place after the listed ones; an unknown
 * id is ignored.
 */
export function reorderItems(
  state: CustomCssState,
  ids: string[],
): CustomCssState {
  const byId = new Map(state.items.map((item) => [item.id, item]))
  const ordered = ids.flatMap((id) => byId.get(id) ?? [])
  const listed = new Set(ids)
  const rest = state.items.filter((item) => !listed.has(item.id))

  return { ...state, items: [...ordered, ...rest] }
}

export function setPresetEnabled(
  state: CustomCssState,
  id: string,
  enabled: boolean,
): CustomCssState {
  const others = state.enabledPresets.filter((presetId) => presetId !== id)

  return { ...state, enabledPresets: enabled ? [...others, id] : others }
}

/**
 * The stylesheets to inject, in cascade order: enabled presets first, in
 * catalog order, then enabled items, in list order, so a user's own CSS wins
 * over a preset. A preset that fails to compile is skipped and reported; the
 * others still apply.
 */
export async function resolveActiveStyles(
  state: CustomCssState,
  presets: readonly CustomCssPreset[],
  compilePreset: (preset: CustomCssPreset) => Promise<string>,
  onError: (preset: CustomCssPreset, error: unknown) => void = () => {},
): Promise<ActiveStyle[]> {
  const enabled = new Set(state.enabledPresets)

  const presetStyles = await Promise.all(
    presets
      .filter((preset) => enabled.has(preset.id))
      .map(async (preset): Promise<ActiveStyle | null> => {
        try {
          return {
            id: customCssStyleId('preset', preset.id),
            css: await compilePreset(preset),
          }
        } catch (error) {
          onError(preset, error)
          return null
        }
      }),
  )

  const itemStyles = state.items
    .filter((item) => item.enabled && item.compiled)
    .map((item) => ({
      id: customCssStyleId('item', item.id),
      css: item.compiled,
    }))

  return [
    ...presetStyles.filter((style): style is ActiveStyle => style !== null),
    ...itemStyles,
  ]
}
