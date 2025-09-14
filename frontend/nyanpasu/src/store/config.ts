import { atom } from 'jotai'
import { atomWithStorage, createJSONStorage } from 'jotai/utils'
import { NyanpasuStorage } from '@/services/storage'

// Current configuration name
export const currentConfigNameAtom = atom<string | null>(null)

// Multi-core warning state
export const multiCoreWarningAtom = atom<boolean>(false)

// Configuration sync status
export const configSyncStatusAtom = atom<{
  isSyncing: boolean
  lastSyncTime: number | null
  error: string | null
}>({
  isSyncing: false,
  lastSyncTime: null,
  error: null
})

// Recovery attempt count
export const recoveryAttemptsAtom = atomWithStorage<number>(
  'config_recovery_attempts',
  0,
  createJSONStorage(() => NyanpasuStorage)
)

// Last successful operation timestamp
export const lastSuccessTimeAtom = atomWithStorage<number | null>(
  'config_last_success_time',
  null,
  createJSONStorage(() => NyanpasuStorage)
)

// Configuration switch history
export const configSwitchHistoryAtom = atomWithStorage<Array<{
  configName: string
  timestamp: number
  success: boolean
}>>(
  'config_switch_history',
  [],
  createJSONStorage(() => NyanpasuStorage)
)

// Proxy groups data
export const proxyGroupsAtom = atom<any[] | null>(null)

// Running cores information
export const runningCoresAtom = atom<Array<{
  type: string
  state: 'Running' | { Stopped: string | null }
  state_changed_at: number
  config_path: string | null
}>>([])