import { createContext, useContext } from 'react'
import type { SectionItems } from './resolve-group-drop'

export type ProxiesDnd = {
  /**
   * The groups of each section by section id, as drag ids. While a group is
   * dragged they follow the drag, so an item shows where it would land.
   */
  items: SectionItems
  /** The folders as drag ids, following a folder drag the same way. */
  folders: string[]
}

export const ProxiesDndContext = createContext<ProxiesDnd | null>(null)

export const useProxiesDnd = () => {
  const context = useContext(ProxiesDndContext)

  if (!context) {
    throw new Error('useProxiesDnd must be used within a ProxiesDndProvider')
  }

  return context
}
