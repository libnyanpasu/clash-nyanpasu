import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type PropsWithChildren,
} from 'react'
import type {
  ClashConnectionDetails_Deserialize,
  ClashConnectionDetails_Serialize,
  ClashConnectionsConnectorState,
  SubscriptionId,
} from '@nyanpasu/rpc/types'
import { Channel, isTauri } from '@tauri-apps/api/core'
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

    if (!isTauri()) {
      let disposed = false
      const source = new EventSource('/bridge/connection-details')
      source.onopen = () => {
        if (disposed) return
        setStatus('connecting')
        setError(null)
      }
      source.onmessage = (event) => {
        if (disposed) return
        try {
          setFrame(JSON.parse(event.data) as ClashConnectionDetails_Serialize)
          setStatus('connected')
          setError(null)
        } catch (error) {
          console.error('failed to decode connection details:', error)
          setError(error)
          setStatus('error')
        }
      }
      source.onerror = (event) => {
        if (disposed) return
        setFrame(null)
        setError(event)
        setStatus('error')
      }
      return () => {
        disposed = true
        source.close()
        setFrame(null)
        setStatus('idle')
        setError(null)
      }
    }

    let disposed = false
    let subscriptionId: SubscriptionId | undefined

    const channel = new Channel<ClashConnectionDetails_Serialize>()
    channel.onmessage = (data) => {
      if (disposed) return
      setFrame(data)
      setStatus('connected')
      setError(null)
    }
    const unsubscribe = (id: SubscriptionId) => {
      rpc
        .unsubscribeClashConnectionDetails(id)
        .then((result) => {
          if (result.status === 'error') {
            console.error(
              'failed to unsubscribe from connection details:',
              result.error,
            )
          }
        })
        .catch((error: unknown) => {
          console.error('failed to unsubscribe from connection details:', error)
        })
    }

    // tauri-specta types every command argument's phase as "Deserialize",
    // including a Channel's payload type, which is actually the direction
    // Rust *serializes* into (ClashConnection's `_extra` is a named field
    // only on that side). The cast documents the mismatch; `channel` is the
    // only thing actually sent across the boundary.
    rpc
      .subscribeClashConnectionDetails(
        channel as unknown as Channel<ClashConnectionDetails_Deserialize>,
      )
      .then((result) => {
        if (disposed) {
          if (result.status === 'ok') unsubscribe(result.data)
          return
        }
        if (result.status === 'error') {
          console.error(
            'failed to subscribe to connection details:',
            result.error,
          )
          setError(result.error)
          setStatus('error')
          return
        }
        subscriptionId = result.data
      })
      .catch((error: unknown) => {
        console.error('failed to subscribe to connection details:', error)
        if (!disposed) {
          setError(error)
          setStatus('error')
        }
      })

    return () => {
      disposed = true
      setFrame(null)
      setStatus('idle')
      setError(null)
      if (subscriptionId !== undefined) {
        unsubscribe(subscriptionId)
      }
    }
  }, [connectorState, hasSubscribers, retrySequence, rpc])

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
