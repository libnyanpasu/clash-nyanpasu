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
import { Channel } from '@tauri-apps/api/core'
import {
  commands,
  events,
  type ClashConnectionsConnectorState,
  type ClashConnectionsSummary,
  type ClashWsEvent,
  type ClashWsKind,
  type ClashWsSnapshot,
  type SubscriptionId,
  type TrafficSummary,
  type TrafficSummaryFrame,
} from '../ipc/bindings'
import type { ClashLog } from '../ipc/use-clash-logs'
import type { ClashMemory } from '../ipc/use-clash-memory'
import type { ClashTraffic } from '../ipc/use-clash-traffic'
import { applyClashWsEvent } from './clash-ws-state'

export type ClashDisplayKind = ClashWsKind | 'connections' | 'traffic'

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
  clearHistory: (kind: ClashDisplayKind) => Promise<void>
}

// One context per history kind: every ws event replaces the snapshot, but a
// consumer only re-renders when its own history or the status changes.
const ClashWSHistoryContexts: {
  [K in ClashDisplayKind]: Context<ClashWSHistory[K] | null>
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

export const useClashWSHistory = <K extends ClashDisplayKind>(kind: K) =>
  useClashWSValue<ClashWSHistory[K]>(ClashWSHistoryContexts[kind])

export const useClashWSStatus = () => useClashWSValue(ClashWSStatusContext)

type ClashWSValues = {
  [K in ClashDisplayKind]: ClashWSHistory[K] | null
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
  const [connectionSnapshots, setConnectionSnapshots] = useState<
    ClashConnectionsSummary[]
  >([])
  const [trafficSnapshots, setTrafficSnapshots] = useState<ClashTraffic[]>([])
  const [trafficState, setTrafficState] =
    useState<ClashConnectionsConnectorState>('disconnected')
  const [trafficError, setTrafficError] = useState<unknown>(null)
  const [trafficLoading, setTrafficLoading] = useState(true)
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

  useEffect(() => {
    let disposed = false
    let subscriptionId: SubscriptionId | undefined
    let session: string | undefined
    let revision = -1n
    const accept = (summary: TrafficSummary | null) => {
      if (disposed) return
      setTrafficLoading(false)
      if (!summary) {
        setTrafficState('disconnected')
        // Only presentation caches are reset when a host stream retires.
        setConnectionSnapshots([])
        setTrafficSnapshots([])
        return
      }
      const changed = session !== summary.session.id
      if (!changed && BigInt(summary.revision) < revision) return
      session = summary.session.id
      revision = BigInt(summary.revision)
      const known = summary.current_rate
      setTrafficState(
        summary.session.freshness === 'Fresh' ? 'connected' : 'disconnected',
      )
      // These numbers feed the existing display only; domain accounting stays decimal strings.
      const frame: ClashConnectionsSummary = {
        downloadTotal: Number(summary.session.core_reported_bytes.download),
        uploadTotal: Number(summary.session.core_reported_bytes.upload),
        downloadSpeed: known?.download ?? 0,
        uploadSpeed: known?.upload ?? 0,
        memory: null,
        connectionCount: Number(summary.active_connections),
        memberRates: Object.fromEntries(
          Object.entries(summary.member_rates).map(([key, rate]) => [
            key,
            { download: rate?.download ?? 0, upload: rate?.upload ?? 0 },
          ]),
        ),
      }
      setConnectionSnapshots((history) =>
        [...(changed ? [] : history), frame].slice(-32),
      )
      setTrafficSnapshots((history) =>
        [
          ...(changed ? [] : history),
          { up: known?.upload ?? 0, down: known?.download ?? 0 },
        ].slice(-32),
      )
    }
    const channel = new Channel<TrafficSummaryFrame>()
    channel.onmessage = (frame) => {
      if (disposed) return
      accept(frame.summary)
      setTrafficError(frame.error)
    }
    commands
      .subscribeTrafficSummary(channel)
      .then((result) => {
        if (result.status === 'error') {
          if (!disposed) {
            setTrafficError(result.error)
            setTrafficLoading(false)
          }
          return
        }
        if (disposed) commands.unsubscribeTrafficSubscription(result.data)
        else subscriptionId = result.data
      })
      .catch((error) => {
        if (!disposed) {
          setTrafficError(error)
          setTrafficLoading(false)
        }
      })
    return () => {
      disposed = true
      if (subscriptionId !== undefined)
        commands.unsubscribeTrafficSubscription(subscriptionId)
    }
  }, [])

  const clearHistory = useCallback(async (kind: ClashDisplayKind) => {
    if (kind === 'connections') {
      setConnectionSnapshots([])
      return
    }
    if (kind === 'traffic') {
      setTrafficSnapshots([])
      return
    }
    const result = await commands.clearClashWsHistory(kind)
    if (result.status === 'error') throw result.error
  }, [])

  const logSnapshots = snapshot?.logs
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

  const state = trafficState
  const status = useMemo(
    () => ({
      isLoading: isLoading || trafficLoading,
      error: error ?? trafficError,
      state,
      clearHistory,
    }),
    [isLoading, trafficLoading, error, trafficError, state, clearHistory],
  )

  const values = useMemo(
    () => ({ connections, logs, traffic, memory, status }),
    [connections, logs, traffic, memory, status],
  )

  return (
    <ClashWSValuesProvider values={values}>{children}</ClashWSValuesProvider>
  )
}
