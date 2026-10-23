import { useCallback } from 'react'
import { useKvStorage } from '@nyanpasu/query'

const COLLAPSED_KV_KEY = 'profiles-folder-collapsed'

/** Sections that are collapsed, by folder id (`ungrouped` for the rest). */
type CollapsedSections = Record<string, true>

const toCollapsedSections = (value: unknown): CollapsedSections => {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    return {}
  }

  return Object.fromEntries(
    Object.entries(value).filter(([, collapsed]) => collapsed === true),
  ) as CollapsedSections
}

/**
 * Which profile sections are collapsed. The state is stored per folder and
 * shared by every window.
 */
export function useFolderCollapsed() {
  const [collapsed, setCollapsed] = useKvStorage<CollapsedSections>(
    COLLAPSED_KV_KEY,
    {},
    { migrate: toCollapsedSections },
  )

  const toggle = useCallback(
    (key: string) =>
      setCollapsed((previous) => {
        const { [key]: wasCollapsed, ...rest } = previous

        return wasCollapsed ? rest : { ...rest, [key]: true }
      }),
    [setCollapsed],
  )

  return { isCollapsed: (key: string) => collapsed[key] === true, toggle }
}
