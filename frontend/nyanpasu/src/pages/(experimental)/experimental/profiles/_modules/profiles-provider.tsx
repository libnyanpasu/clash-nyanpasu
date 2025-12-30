import { createContext, PropsWithChildren, useContext, useState } from 'react'

const ProfilesContext = createContext<{
  sidebarOpen: boolean
  setSidebarOpen: (value: boolean) => void
} | null>(null)

export const useProfilesContext = () => {
  const context = useContext(ProfilesContext)

  if (!context) {
    throw new Error('useProfilesContext must be used within a ProfilesProvider')
  }

  return context
}

export const ProfilesProvider = ({ children }: PropsWithChildren) => {
  const [sidebarOpen, setSidebarOpen] = useState(false)

  return (
    <ProfilesContext.Provider
      value={{
        sidebarOpen,
        setSidebarOpen,
      }}
    >
      {children}
    </ProfilesContext.Provider>
  )
}
