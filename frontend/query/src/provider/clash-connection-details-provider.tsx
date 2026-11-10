import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type PropsWithChildren,
} from 'react'
import { useRuntime } from '@nyanpasu/platform/react'
import { createConnectionDetailsTransport } from '@nyanpasu/rpc'
import type {
  ClashConnectionDetails_Deserialize,
  ClashConnectionDetails_Serialize,
  ClashConnectionsConnectorState,
  SubscriptionId,
} from '@nyanpasu/rpc/types'
import { useRpc } from './rpc-provider'

// Registers one consumer and returns its unregister; only the 0→1 and 1→0
// transitions of the resulting count (not every register/unregister) drive
// the subscribe/unsubscribe IPC calls, so N consumers share one subscription.
type Registrar = () => () => void

const RegistrarContext = createContext<Registrar | null>(null)
type ConnectionDetailsState = {
  frame: ClashConnectionDetails_Serialize | null
  status: 'idle' | 'connecting' | 'connected' | 'error'
  connectorState: ClashConnectionsConnectorState
  error: unknown
  retry: () => void
}

// `frame: null` means no frame yet, never "no provider": that is
// `RegistrarContext`'s job, checked once in `useClashConnectionDetails`.
const FrameContext = createContext<ConnectionDetailsState | null>(null)

const useFrameValue = () => {
  const value = useContext(FrameContext)
  if (!value) throw new Error('Connection details must be within its provider')
  return value
}

/** Latest detail frame plus the state of the shared detail subscription. */
export const useClashConnectionDetails = () => {
  const registrar = useContext(RegistrarContext)
  const { frame, status, connectorState, error, retry } = useFrameValue()

  useEffect(() => (registrar ? registrar() : undefined), [registrar])

  if (!registrar) {
    throw new Error(
      'useClashConnectionDetails must be used within a ClashConnectionDetailsProvider',
    )
  }

  return {
    data: frame,
    isLoading: frame === null,
    status,
    connectorState,
    error,
    retry,
  }
}

// Mirrors `ClashWSFreezeBoundary` (see clash-ws-provider.tsx): while frozen,
// re-provides the frame captured when freezing started, so an exiting page
// stops re-rendering on every detail frame.
export const ClashConnectionDetailsFreezeBoundary = ({
  frozen,
  children,
}: PropsWithChildren<{ frozen: boolean }>) => {
  const live = useFrameValue()
  // Wrapped so a legitimately `null` (loading) frame can still be told apart
  // from "not currently capturing" (`captured === null`).
  const [captured, setCaptured] = useState<{
    value: ConnectionDetailsState
  } | null>(null)

  if (frozen && captured === null) {
    setCaptured({ value: live })
  } else if (!frozen && captured !== null) {
    setCaptured(null)
  }

  return (
    // The render that starts freezing still sees `captured === null` (the
    // state update above only applies on the re-render), so it provides
    // `live`, which is the value being captured.
    <FrameContext.Provider value={frozen && captured ? captured.value : live}>
      {children}
    </FrameContext.Provider>
  )
}

export const ClashConnectionDetailsProvider = ({
  connectorState,
  children,
}: PropsWithChildren<{
  /**
   * The underlying `ClashWSProvider`'s connector state (`useClashWSStatus()`
   * there). Taken as a prop rather than read directly, so this provider
   * neither depends on nor needs `ClashWSProvider` mounted to be tested.
   */
  connectorState: ClashConnectionsConnectorState
}>) => {
  const rpc = useRpc()
  const runtime = useRuntime()
  const [subscriberCount, setSubscriberCount] = useState(0)
  const [frame, setFrame] = useState<ClashConnectionDetails_Serialize | null>(
    null,
  )
  const [status, setStatus] = useState<ConnectionDetailsState['status']>('idle')
  const [error, setError] = useState<unknown>(null)
  const [retrySequence, setRetrySequence] = useState(0)

  const register = useCallback<Registrar>(() => {
    setSubscriberCount((count) => count + 1)
    return () => setSubscriberCount((count) => count - 1)
  }, [])

  const retry = useCallback(() => {
    setRetrySequence((sequence) => sequence + 1)
  }, [])

  // The Rust side clears its own watch to `None` on disconnect, but the
  // forwarder only ever forwards `Some` frames, so a webview learns of a
  // disconnect only by watching the connector state itself, not the channel.
  useEffect(() => {
    if (connectorState !== 'connected') {
      setFrame(null)
      setStatus('idle')
      setError(null)
    }
  }, [connectorState])

  const hasSubscribers = subscriberCount > 0

  useEffect(() => {
    if (!hasSubscribers || connectorState !== 'connected') {
      if (!hasSubscribers) {
        setStatus('idle')
        setError(null)
      }
      return
    }

    setStatus('connecting')
    setError(null)
    setFrame(null)

    const transport =
      createConnectionDetailsTransport<ClashConnectionDetails_Serialize>(
        runtime,
        {
          subscribe: (channel) =>
            rpc.subscribeClashConnectionDetails(
              channel as unknown as import('@tauri-apps/api/core').Channel<ClashConnectionDetails_Deserialize>,
            ),
          unsubscribe: (id) =>
            rpc.unsubscribeClashConnectionDetails(id as SubscriptionId),
        },
      )
    const stop = transport.listen({
      onOpen: () => {
        setStatus('connecting')
        setError(null)
      },
      onMessage: (data) => {
        setFrame(data)
        setStatus('connected')
        setError(null)
      },
      onError: (error) => {
        setFrame(null)
        setError(error)
        setStatus('error')
      },
    })
    return () => {
      stop()
      setFrame(null)
      setStatus('idle')
      setError(null)
    }
  }, [connectorState, hasSubscribers, retrySequence, rpc, runtime])

  const value = useMemo(
    () => ({ frame, status, connectorState, error, retry }),
    [connectorState, error, frame, retry, status],
  )

  return (
    <RegistrarContext.Provider value={register}>
      <FrameContext.Provider value={value}>{children}</FrameContext.Provider>
    </RegistrarContext.Provider>
  )
}
