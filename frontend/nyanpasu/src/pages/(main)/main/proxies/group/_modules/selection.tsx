import {
  createContext,
  PropsWithChildren,
  useCallback,
  useContext,
  useMemo,
  useState,
} from 'react'

type GroupSelectionContextValue = {
  /** Whether the node multi-select mode is active. */
  selecting: boolean
  /** Currently selected node names. */
  selected: Set<string>
  count: number
  isSelected: (name: string) => boolean
  toggle: (name: string) => void
  /** Enter selection mode, optionally pre-selecting a node. */
  enter: (initial?: string) => void
  /** Leave selection mode and clear the selection. */
  exit: () => void
  clear: () => void
  selectAll: (names: string[]) => void
  /** Create-group dialog visibility (shared by the action bar & context menu). */
  createOpen: boolean
  openCreate: () => void
  setCreateOpen: (open: boolean) => void
}

const GroupSelectionContext = createContext<GroupSelectionContextValue | null>(
  null,
)

export const useGroupSelection = () => {
  const context = useContext(GroupSelectionContext)

  if (!context) {
    throw new Error(
      'useGroupSelection must be used within a GroupSelectionProvider',
    )
  }

  return context
}

export function GroupSelectionProvider({ children }: PropsWithChildren) {
  const [selecting, setSelecting] = useState(false)
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [createOpen, setCreateOpen] = useState(false)

  const toggle = useCallback((name: string) => {
    setSelected((prev) => {
      const next = new Set(prev)

      if (next.has(name)) {
        next.delete(name)
      } else {
        next.add(name)
      }

      return next
    })
  }, [])

  const enter = useCallback((initial?: string) => {
    setSelecting(true)
    setSelected(initial ? new Set([initial]) : new Set())
  }, [])

  const exit = useCallback(() => {
    setSelecting(false)
    setSelected(new Set())
    setCreateOpen(false)
  }, [])

  const clear = useCallback(() => setSelected(new Set()), [])

  const selectAll = useCallback(
    (names: string[]) => setSelected(new Set(names)),
    [],
  )

  const isSelected = useCallback(
    (name: string) => selected.has(name),
    [selected],
  )

  const openCreate = useCallback(() => setCreateOpen(true), [])

  const value = useMemo<GroupSelectionContextValue>(
    () => ({
      selecting,
      selected,
      count: selected.size,
      isSelected,
      toggle,
      enter,
      exit,
      clear,
      selectAll,
      createOpen,
      openCreate,
      setCreateOpen,
    }),
    [
      selecting,
      selected,
      isSelected,
      toggle,
      enter,
      exit,
      clear,
      selectAll,
      createOpen,
      openCreate,
    ],
  )

  return (
    <GroupSelectionContext.Provider value={value}>
      {children}
    </GroupSelectionContext.Provider>
  )
}
