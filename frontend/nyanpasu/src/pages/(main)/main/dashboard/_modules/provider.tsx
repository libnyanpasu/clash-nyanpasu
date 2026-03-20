import { createContext, PropsWithChildren, use, useState } from 'react'

const DashboardContext = createContext<{
  openSheet: boolean
  setOpenSheet: (open: boolean) => void
  isEditing: boolean
  setIsEditing: (editing: boolean) => void
} | null>(null)

export const useDashboardContext = () => {
  const context = use(DashboardContext)

  if (!context) {
    throw new Error(
      'useDashboardContext must be used within a DashboardProvider',
    )
  }

  return context
}

export function DashboardProvider({ children }: PropsWithChildren) {
  const [openSheet, setOpenSheet] = useState(false)

  const [isEditing, setIsEditing] = useState(false)

  return (
    <DashboardContext.Provider
      value={{
        openSheet,
        setOpenSheet,
        isEditing,
        setIsEditing,
      }}
    >
      {children}
    </DashboardContext.Provider>
  )
}
