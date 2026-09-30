import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type Context,
  type PropsWithChildren,
} from 'react'
import { rpc } from '../ipc/rpc'
import {
  type ClashConnectionsConnectorState,
  type ClashConnectionsSummary,
  type ClashWsEvent,
  type ClashWsKind,
  type ClashWsSnapshot,
} from '../ipc/rpc-bindings'
import type { ClashLog } from '../ipc/use-clash-logs'
import type { ClashMemory } from '../ipc/use-clash-memory'
import type { ClashTraffic } from '../ipc/use-clash-traffic'
import { applyClashWsEvent } from './clash-ws-state'

type ClashWSHistory = {
  connections: ClashConnectionsSummary[]
  logs: ClashLog[]
  traffic: ClashTraffic[]
  memory: ClashMemory[]
}

type ClashWSStatus = {
  isLoading: boolean
  error: unknown
  state: ClashConnectionsConnectorState
  clearHistory: (kind: ClashWsKind) => Promise<void>
}

// One context per history kind: every ws event replaces the snapshot, but a
// consumer only re-renders when its own history or the status changes.
const ClashWSHistoryContexts: {
  [K in ClashWsKind]: Context<ClashWSHistory[K] | null>
} = {
  connections: createContext<ClashConnectionsSummary[] | null>(null),
  logs: createContext<ClashLog[] | null>(null),
  traffic: createContext<ClashTraffic[] | null>(null),
  memory: createContext<ClashMemory[] | null>(null),
}

const ClashWSStatusContext = createContext<ClashWSStatus | null>(null)

const useClashWSValue = <T,>(context: Context<T | null>) => {
  const value = useContext(context)

  if (value === null) {
    throw new Error('Clash ws hooks must be used in a ClashWSProvider')
  }

  return value
}

export const useClashWSHistory = <K extends ClashWsKind>(kind: K) =>
  useClashWSValue<ClashWSHistory[K]>(ClashWSHistoryContexts[kind])

export const useClashWSStatus = () => useClashWSValue(ClashWSStatusContext)

type ClashWSValues = {
  [K in ClashWsKind]: ClashWSHistory[K] | null
} & {
  status: ClashWSStatus | null
}

const ClashWSValuesProvider = ({
  values,
  children,
}: PropsWithChildren<{ values: ClashWSValues }>) => (
  <ClashWSStatusContext.Provider value={values.status}>
    <ClashWSHistoryContexts.connections.Provider value={values.connections}>
      <ClashWSHistoryContexts.logs.Provider value={values.logs}>
        <ClashWSHistoryContexts.traffic.Provider value={values.traffic}>
          <ClashWSHistoryContexts.memory.Provider value={values.memory}>
            {children}
          </ClashWSHistoryContexts.memory.Provider>
        </ClashWSHistoryContexts.traffic.Provider>
      </ClashWSHistoryContexts.logs.Provider>
    </ClashWSHistoryContexts.connections.Provider>
  </ClashWSStatusContext.Provider>
)

// While `frozen`, re-provides the values captured when it turned on, so a
// subtree that is on its way out (a page animating away) stops re-rendering
// on every sample. Keep it mounted and toggle `frozen`: inserting it only
// when freezing would remount the subtree.
export const ClashWSFreezeBoundary = ({
  frozen,
  children,
}: PropsWithChildren<{ frozen: boolean }>) => {
  const live: ClashWSValues = {
    connections: useContext(ClashWSHistoryContexts.connections),
    logs: useContext(ClashWSHistoryContexts.logs),
    traffic: useContext(ClashWSHistoryContexts.traffic),
    memory: useContext(ClashWSHistoryContexts.memory),
    status: useContext(ClashWSStatusContext),
  }

  const [captured, setCaptured] = useState<ClashWSValues | null>(null)

  if (frozen && captured === null) {
    setCaptured(live)
  } else if (!frozen && captured !== null) {
    setCaptured(null)
  }

  return (
    <ClashWSValuesProvider values={(frozen && captured) || live}>
      {children}
    </ClashWSValuesProvider>
  )
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
          const result = await rpc.getClashWsSnapshot()
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
    rpc.events.clashWsEvent
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

    const stopResync = rpc.listenResync(resync)

    return () => {
      stopResync()
      disposed = true
      unlisten?.()
    }
  }, [])

  const clearHistory = useCallback(async (kind: ClashWsKind) => {
    const result = await rpc.clearClashWsHistory(kind)
    if (result.status === 'error') throw result.error
    // The sequenced history_cleared event orders this against later samples.
  }, [])

  // Snapshot updates keep the arrays of untouched kinds, so memoizing on them
  // keeps each history context value stable across unrelated rpc.events.
  const connectionSnapshots = snapshot?.connections
  const logSnapshots = snapshot?.logs
  const trafficSnapshots = snapshot?.traffic
  const memorySnapshots = snapshot?.memory

  const connections = useMemo(
    () => connectionSnapshots ?? [],
    [connectionSnapshots],
  )
  const logs = useMemo(() => (logSnapshots ?? []) as ClashLog[], [logSnapshots])
  const traffic = useMemo(
    () => (trafficSnapshots ?? []) as ClashTraffic[],
    [trafficSnapshots],
  )
  const memory = useMemo(
    () => (memorySnapshots ?? []) as ClashMemory[],
    [memorySnapshots],
  )

  const state = snapshot?.state ?? 'disconnected'
  const status = useMemo(
    () => ({ isLoading, error, state, clearHistory }),
    [isLoading, error, state, clearHistory],
  )

  const values = useMemo(
    () => ({ connections, logs, traffic, memory, status }),
    [connections, logs, traffic, memory, status],
  )

  return (
    <ClashWSValuesProvider values={values}>{children}</ClashWSValuesProvider>
  )
}
