import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
  type PropsWithChildren,
} from 'react'
import { commands, events, type ClashWsKind } from '../ipc/bindings'
import {
  MAX_CONNECTIONS_HISTORY,
  MAX_LOGS_HISTORY,
  MAX_MEMORY_HISTORY,
  MAX_TRAFFIC_HISTORY,
} from '../ipc/consts'
import type { ClashConnection } from '../ipc/use-clash-connections'
import type { ClashLog } from '../ipc/use-clash-logs'
import type { ClashMemory } from '../ipc/use-clash-memory'
import type { ClashTraffic } from '../ipc/use-clash-traffic'

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

const queryKeyForKind = (kind: ClashWsKind) => {
  switch (kind) {
    case 'connections':
      return 'connections'
    case 'logs':
      return 'logs'
    case 'traffic':
      return 'traffic'
    case 'memory':
      return 'memory'
  }
}

const appendLimited = <T,>(items: T[] | undefined, item: T, limit: number) => {
  const next = [...(items || []), item]

  if (next.length > limit) {
    next.shift()
  }

  return next
}

export const ClashWSProvider = ({ children }: PropsWithChildren) => {
  const [connections, setConnections] = useState<ClashConnection[]>([])
  const [logs, setLogs] = useState<ClashLog[]>([])
  const [traffic, setTraffic] = useState<ClashTraffic[]>([])
  const [memory, setMemory] = useState<ClashMemory[]>([])
  const [isLoading, setIsLoading] = useState(true)
  const [error, setError] = useState<unknown>(null)

  useEffect(() => {
    commands.getClashWsSnapshot().then((result) => {
      if (result.status === 'error') {
        console.error('Failed to load clash websocket snapshot', result.error)
        setError(result.error)
        setIsLoading(false)
        return
      }

      const snapshot = result.data
      setConnections(
        snapshot.connections.map((connection) => ({
          ...connection,
          memory: connection.memory ?? undefined,
          connections:
            (connection.connections as ClashConnection['connections']) ??
            undefined,
        })),
      )
      setLogs(snapshot.logs as ClashLog[])
      setTraffic(snapshot.traffic as ClashTraffic[])
      setMemory(snapshot.memory as ClashMemory[])
      setIsLoading(false)
    })
  }, [])

  useEffect(() => {
    const unlistenPromise = events.clashWsEvent.listen((event) => {
      const payload = event.payload

      switch (payload.kind) {
        case 'connections_updated': {
          setConnections((current) =>
            appendLimited(
              current,
              {
                ...payload.data,
                memory: payload.data.memory ?? undefined,
                connections:
                  (payload.data
                    .connections as ClashConnection['connections']) ??
                  undefined,
              },
              MAX_CONNECTIONS_HISTORY,
            ),
          )
          break
        }
        case 'log_appended': {
          setLogs((current) =>
            appendLimited(current, payload.data as ClashLog, MAX_LOGS_HISTORY),
          )
          break
        }
        case 'traffic_updated': {
          setTraffic((current) =>
            appendLimited(
              current,
              payload.data as ClashTraffic,
              MAX_TRAFFIC_HISTORY,
            ),
          )
          break
        }
        case 'memory_updated': {
          setMemory((current) =>
            appendLimited(
              current,
              payload.data as ClashMemory,
              MAX_MEMORY_HISTORY,
            ),
          )
          break
        }
        case 'recording_changed':
          break
        case 'history_cleared': {
          switch (queryKeyForKind(payload.data)) {
            case 'connections':
              setConnections([])
              break
            case 'logs':
              setLogs([])
              break
            case 'traffic':
              setTraffic([])
              break
            case 'memory':
              setMemory([])
              break
          }
          break
        }
        case 'state_changed':
          break
      }
    })

    return () => {
      unlistenPromise.then((unlisten) => unlisten())
    }
  }, [])

  const clearHistory = useCallback(async (kind: ClashWsKind) => {
    const result = await commands.clearClashWsHistory(kind)

    if (result.status === 'error') {
      throw result.error
    }

    switch (kind) {
      case 'connections':
        setConnections([])
        break
      case 'logs':
        setLogs([])
        break
      case 'traffic':
        setTraffic([])
        break
      case 'memory':
        setMemory([])
        break
    }
  }, [])

  return (
    <ClashWSContext.Provider
      value={{
        connections,
        logs,
        traffic,
        memory,
        isLoading,
        error,
        clearHistory,
      }}
    >
      {children}
    </ClashWSContext.Provider>
  )
}
