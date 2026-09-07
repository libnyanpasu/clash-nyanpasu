import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
  type PropsWithChildren,
} from 'react'
import {
  commands,
  events,
  type ClashWsEvent,
  type ClashWsKind,
  type ClashWsSnapshot,
} from '../ipc/bindings'
import type { ClashConnection } from '../ipc/use-clash-connections'
import type { ClashLog } from '../ipc/use-clash-logs'
import type { ClashMemory } from '../ipc/use-clash-memory'
import type { ClashTraffic } from '../ipc/use-clash-traffic'
import { applyClashWsEvent } from './clash-ws-state'

const ClashWSContext = createContext<{
  connections: ClashConnection[]
  logs: ClashLog[]
  traffic: ClashTraffic[]
  memory: ClashMemory[]
  isLoading: boolean
  error: unknown
  clearHistory: (kind: ClashWsKind) => Promise<void>
} | null>(null)

export const useClashWSContext = () => {
  const context = useContext(ClashWSContext)

  if (!context) {
    throw new Error('useClashWSContext must be used in a ClashWSProvider')
  }

  return context
}

export const ClashWSProvider = ({ children }: PropsWithChildren) => {
  const [snapshot, setSnapshot] = useState<ClashWsSnapshot>()
  const [isLoading, setIsLoading] = useState(true)
  const [error, setError] = useState<unknown>(null)

  useEffect(() => {
    let disposed = false
    let current: ClashWsSnapshot | undefined
    let syncing = false
    let pending: ClashWsEvent[] = []
    let unlisten: (() => void) | undefined

    const resync = async () => {
      if (syncing || disposed) return
      syncing = true
      try {
        do {
          const result = await commands.getClashWsSnapshot()
          if (disposed) return
          if (result.status === 'error') throw result.error
          if (!current || result.data.sequence >= current.sequence)
            current = result.data
          const buffered = pending
          pending = []
          let gap = false
          for (const event of buffered) {
            const next = applyClashWsEvent(current, event)
            if (!next) {
              gap = true
              break
            }
            current = next
          }
          if (!gap) break
        } while (!disposed)
        setSnapshot(current)
        setError(null)
      } catch (error) {
        if (!disposed) setError(error)
      } finally {
        syncing = false
        if (!disposed) setIsLoading(false)
      }
    }

    // Subscribe before requesting the snapshot. The bounded buffer plus sequence
    // checks also covers slow IPC, event loss, and StrictMode effect teardown.
    events.clashWsEvent
      .listen(({ payload }) => {
        if (disposed) return
        if (syncing || !current) {
          pending = [...pending, payload].slice(-256)
          resync()
          return
        }
        const next = applyClashWsEvent(current, payload)
        if (!next) {
          pending = [payload]
          resync()
          return
        }
        current = next
        setSnapshot(next)
      })
      .then((stop) => {
        if (disposed) {
          stop()
          return
        }
        unlisten = stop
        resync()
      })
      .catch((error) => {
        if (!disposed) {
          setError(error)
          setIsLoading(false)
        }
      })

    return () => {
      disposed = true
      unlisten?.()
    }
  }, [])

  const clearHistory = useCallback(async (kind: ClashWsKind) => {
    const result = await commands.clearClashWsHistory(kind)
    if (result.status === 'error') throw result.error
    // The sequenced history_cleared event orders this against later samples.
  }, [])

  const connections: ClashConnection[] = (snapshot?.connections ?? []).map(
    (connection) => ({
      ...connection,
      memory: connection.memory ?? undefined,
      connections:
        (connection.connections as ClashConnection['connections']) ?? undefined,
    }),
  )

  return (
    <ClashWSContext.Provider
      value={{
        connections,
        logs: (snapshot?.logs ?? []) as ClashLog[],
        traffic: (snapshot?.traffic ?? []) as ClashTraffic[],
        memory: (snapshot?.memory ?? []) as ClashMemory[],
        isLoading,
        error,
        clearHistory,
      }}
    >
      {children}
    </ClashWSContext.Provider>
  )
}
