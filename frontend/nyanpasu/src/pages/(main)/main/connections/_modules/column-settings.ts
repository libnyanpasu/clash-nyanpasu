import {
  functionalUpdate,
  type ColumnOrderState,
  type ColumnVisibilityState,
  type Updater,
} from '@tanstack/react-table'
import { useLocalStorage } from '@uidotdev/usehooks'

type ColumnSettings = {
  order: ColumnOrderState
  visibility: ColumnVisibilityState
}

const DEFAULT_COLUMN_SETTINGS: ColumnSettings = { order: [], visibility: {} }

// Saved settings outlive the columns they name: ids that are gone are dropped,
// and columns added since follow the saved ones in their default order.
export function resolveColumnOrder(
  saved: readonly string[],
  ids: readonly string[],
): string[] {
  const known = new Set(ids)
  const kept = [...new Set(saved)].filter((id) => known.has(id))
  const placed = new Set(kept)

  return [...kept, ...ids.filter((id) => !placed.has(id))]
}

export function useColumnSettings(storageKey: string, ids: readonly string[]) {
  const [settings, setSettings] = useLocalStorage<ColumnSettings>(
    storageKey,
    DEFAULT_COLUMN_SETTINGS,
  )

  // Storage is editable by hand, so a missing field falls back to its default.
  const { order: savedOrder = [], visibility = {} } = settings ?? {}
  const order = resolveColumnOrder(savedOrder, ids)

  return {
    order,
    visibility,
    setOrder: (updater: Updater<ColumnOrderState>) =>
      setSettings({ order: functionalUpdate(updater, order), visibility }),
    setVisibility: (updater: Updater<ColumnVisibilityState>) =>
      setSettings({ order, visibility: functionalUpdate(updater, visibility) }),
  }
}
