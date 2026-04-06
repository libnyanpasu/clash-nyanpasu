import { useUpdateEffect } from 'ahooks'
import dayjs from 'dayjs'
import {
  createContext,
  useContext,
  useState,
  type PropsWithChildren,
} from 'react'
import { useQueryClient } from '@tanstack/react-query'
import {
  CLASH_CONNECTIONS_QUERY_KEY,
  CLASH_LOGS_QUERY_KEY,
  CLASH_MEMORY_QUERY_KEY,
  CLASH_TRAAFFIC_QUERY_KEY,
  MAX_CONNECTIONS_HISTORY,
  MAX_LOGS_HISTORY,
  MAX_MEMORY_HISTORY,
  MAX_TRAFFIC_HISTORY,
} from '../ipc/consts'
import type { ClashConnection } from '../ipc/use-clash-connections'
import type { ClashLog } from '../ipc/use-clash-logs'
import type { ClashMemory } from '../ipc/use-clash-memory'
import type { ClashTraffic } from '../ipc/use-clash-traffic'
import { useClashWebSocket } from '../ipc/use-clash-web-socket'

// Utility functions for localStorage persistence
const createPersistedState = (key: string, defaultValue: boolean) => {
  const getStoredValue = (): boolean => {
    try {
      const item = localStorage.getItem(key)
      return item ? JSON.parse(item) : defaultValue
    } catch {
      return defaultValue
    }
  }

  const setStoredValue = (value: boolean) => {
    try {
      localStorage.setItem(key, JSON.stringify(value))
    } catch {
      // Ignore storage errors
    }
  }

  return { getStoredValue, setStoredValue }
}

const MAX_REASONABLE_MEMORY_BYTES = 16 * 1024 ** 4 // 16 TB

const toNonNegativeFiniteNumber = (value: unknown): number | null => {
  const parsed =
    typeof value === 'number'
      ? value
      : typeof value === 'string'
        ? Number(value)
        : NaN

  if (!Number.isFinite(parsed) || parsed < 0) {
    return null
  }

  return parsed
}

const normalizeClashMemory = (raw: unknown): ClashMemory | null => {
  if (typeof raw !== 'object' || raw === null) {
    return null
  }

  const data = raw as Record<string, unknown>
  let inuse = toNonNegativeFiniteNumber(data.inuse)
  let oslimit = toNonNegativeFiniteNumber(data.oslimit) ?? 0

  if (inuse === null) {
    return null
  }

  // Keep memory values in bytes and normalize obvious unit mismatches.
  if (oslimit > 0 && inuse > oslimit * 2) {
    if (inuse / 8 <= oslimit * 2) {
      inuse /= 8
    }

    while (inuse > oslimit * 2 && inuse % 1024 === 0) {
      inuse /= 1024
    }

    if (inuse > oslimit * 2) {
      inuse = oslimit
    }
  } else if (oslimit <= 0 && inuse > MAX_REASONABLE_MEMORY_BYTES) {
    return null
  }

  return {
    inuse: Math.trunc(inuse),
    oslimit: Math.trunc(oslimit),
  }
}

const ClashWSContext = createContext<{
  recordLogs: boolean
  setRecordLogs: (value: boolean) => void
  recordTraffic: boolean
  setRecordTraffic: (value: boolean) => void
  recordMemory: boolean
  setRecordMemory: (value: boolean) => void
  recordConnections: boolean
  setRecordConnections: (value: boolean) => void
} | null>(null)

export const useClashWSContext = () => {
  const context = useContext(ClashWSContext)

  if (!context) {
    throw new Error('useClashWSContext must be used in a ClashWSProvider')
  }

  return context
}

export const ClashWSProvider = ({ children }: PropsWithChildren) => {
  // Create persisted state handlers
  const logsStorage = createPersistedState('clash-ws-record-logs', true)
  const trafficStorage = createPersistedState('clash-ws-record-traffic', true)
  const memoryStorage = createPersistedState('clash-ws-record-memory', true)
  const connectionsStorage = createPersistedState(
    'clash-ws-record-connections',
    true,
  )

  // Initialize states with persisted values
  const [recordLogs, setRecordLogsState] = useState(logsStorage.getStoredValue)
  const [recordTraffic, setRecordTrafficState] = useState(
    trafficStorage.getStoredValue,
  )
  const [recordMemory, setRecordMemoryState] = useState(
    memoryStorage.getStoredValue,
  )
  const [recordConnections, setRecordConnectionsState] = useState(
    connectionsStorage.getStoredValue,
  )

  // Wrapped setters that also persist to localStorage
  const setRecordLogs = (value: boolean) => {
    setRecordLogsState(value)
    logsStorage.setStoredValue(value)
  }

  const setRecordTraffic = (value: boolean) => {
    setRecordTrafficState(value)
    trafficStorage.setStoredValue(value)
  }

  const setRecordMemory = (value: boolean) => {
    setRecordMemoryState(value)
    memoryStorage.setStoredValue(value)
  }

  const setRecordConnections = (value: boolean) => {
    setRecordConnectionsState(value)
    connectionsStorage.setStoredValue(value)
  }

  const { connectionsWS, memoryWS, trafficWS, logsWS } = useClashWebSocket()

  const queryClient = useQueryClient()

  // clash connections
  useUpdateEffect(() => {
    if (!recordConnections) {
      return
    }

    const data = JSON.parse(
      connectionsWS.latestMessage?.data,
    ) as ClashConnection

    const currentData = queryClient.getQueryData([
      CLASH_CONNECTIONS_QUERY_KEY,
    ]) as ClashConnection[]

    const newData = [...(currentData || []), data]

    if (newData.length > MAX_CONNECTIONS_HISTORY) {
      newData.shift()
    }

    queryClient.setQueryData([CLASH_CONNECTIONS_QUERY_KEY], newData)
  }, [connectionsWS.latestMessage])

  // clash memory
  useUpdateEffect(() => {
    if (!recordMemory) {
      return
    }

    const rawData = JSON.parse(memoryWS.latestMessage?.data ?? 'null')
    const data = normalizeClashMemory(rawData)

    if (!data) {
      return
    }

    const currentData = queryClient.getQueryData([
      CLASH_MEMORY_QUERY_KEY,
    ]) as ClashMemory[]

    const newData = [...(currentData || []), data]

    if (newData.length > MAX_MEMORY_HISTORY) {
      newData.shift()
    }

    queryClient.setQueryData([CLASH_MEMORY_QUERY_KEY], newData)
  }, [memoryWS.latestMessage])

  // clash traffic
  useUpdateEffect(() => {
    if (!recordTraffic) {
      return
    }

    const data = JSON.parse(trafficWS.latestMessage?.data) as ClashTraffic

    const currentData = queryClient.getQueryData([
      CLASH_TRAAFFIC_QUERY_KEY,
    ]) as ClashTraffic[]

    const newData = [...(currentData || []), data]

    if (newData.length > MAX_TRAFFIC_HISTORY) {
      newData.shift()
    }

    queryClient.setQueryData([CLASH_TRAAFFIC_QUERY_KEY], newData)
  }, [trafficWS.latestMessage])

  // clash logs
  useUpdateEffect(() => {
    if (!recordLogs) {
      return
    }

    const data = {
      ...JSON.parse(logsWS.latestMessage?.data),
      time: dayjs(new Date()).format('HH:mm:ss'),
    } as ClashLog

    const currentData = queryClient.getQueryData([
      CLASH_LOGS_QUERY_KEY,
    ]) as ClashLog[]

    const newData = [...(currentData || []), data]

    if (newData.length > MAX_LOGS_HISTORY) {
      newData.shift()
    }

    queryClient.setQueryData([CLASH_LOGS_QUERY_KEY], newData)
  }, [logsWS.latestMessage])

  return (
    <ClashWSContext.Provider
      value={{
        recordLogs,
        setRecordLogs,
        recordTraffic,
        setRecordTraffic,
        recordMemory,
        setRecordMemory,
        recordConnections,
        setRecordConnections,
      }}
    >
      {children}
    </ClashWSContext.Provider>
  )
}
