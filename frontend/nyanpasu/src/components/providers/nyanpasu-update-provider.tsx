import { createContext, PropsWithChildren, use } from 'react'
import { useAppUpdate } from '@nyanpasu/query'
import packageJson from '@root/package.json'
import { isTauri } from '@tauri-apps/api/core'

const NyanpasuUpdateContext = createContext<
  | (ReturnType<typeof useAppUpdate> & {
      currentVersion: string
      hasNewVersion: boolean
      isChecking: boolean
      isInstalling: boolean
      isBusy: boolean
      isDesktop: boolean
      isReady: boolean
    })
  | null
>(null)

export const useNyanpasuUpdate = () => {
  const context = use(NyanpasuUpdateContext)

  if (!context) {
    throw new Error(
      'useNyanpasuUpdate must be used within a NyanpasuUpdateProvider',
    )
  }

  return context
}

export default function NyanpasuUpdateProvider({
  children,
}: PropsWithChildren) {
  const isDesktop = isTauri()
  const update = useAppUpdate({ enabled: isDesktop })
  const phase = update.snapshot?.phase
  const isBusy =
    phase === 'checking' ||
    phase === 'downloading' ||
    phase === 'cancelling' ||
    phase === 'verifying' ||
    phase === 'installing'
  const hasNewVersion = Boolean(
    update.snapshot?.release && phase !== 'up_to_date' && phase !== 'idle',
  )
  return (
    <NyanpasuUpdateContext.Provider
      value={{
        ...update,
        currentVersion: packageJson.version,
        hasNewVersion,
        isChecking: phase === 'checking',
        isInstalling: phase === 'installing',
        isBusy,
        isDesktop,
        isReady: phase === 'ready',
      }}
    >
      {children}
    </NyanpasuUpdateContext.Provider>
  )
}
