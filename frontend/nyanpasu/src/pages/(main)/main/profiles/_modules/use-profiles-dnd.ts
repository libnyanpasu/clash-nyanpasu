import { createContext, useContext } from 'react'
import type { SectionItems } from './resolve-profile-drop'

export type ProfilesDnd = {
  /**
   * The profiles of each section by section id. While a profile is dragged
   * they follow the drag, so a card shows where it would land.
   */
  items: SectionItems
}

export const ProfilesDndContext = createContext<ProfilesDnd | null>(null)

export const useProfilesDnd = () => {
  const context = useContext(ProfilesDndContext)

  if (!context) {
    throw new Error('useProfilesDnd must be used within a ProfilesDndProvider')
  }

  return context
}
