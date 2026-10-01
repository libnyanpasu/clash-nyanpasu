import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
  type PropsWithChildren,
} from 'react'
import { Channel, isTauri } from '@tauri-apps/api/core'
import { rpc } from '../ipc/rpc'
import type {
  ClashConnectionDetails_Deserialize,
  ClashConnectionDetails_Serialize,
  ClashConnectionsConnectorState,
  SubscriptionId,
} from '../ipc/rpc-bindings'

// Registers one consumer and returns its unregister; only the 0→1 and 1→0
// transitions of the resulting count (not every register/unregister) drive
// the subscribe/unsubscribe IPC calls, so N consumers share one subscription.
type Registrar = () => () => void

const RegistrarContext = createContext<Registrar | null>(null)
// `null` here means "no frame yet" (loading), never "no provider": that is
// `RegistrarContext`'s job, checked once in `useClashConnectionDetails`.
const FrameContext = createContext<ClashConnectionDetails_Serialize | null>(
  null,
)

const useFrameValue = () => useContext(FrameContext)

/** Latest connection-detail frame, or `null` before the first one arrives. */
export const useClashConnectionDetails = () => {
  const registrar = useContext(RegistrarContext)
  const frame = useFrameValue()

  useEffect(() => (registrar ? registrar() : undefined), [registrar])

  if (!registrar) {
    throw new Error(
      'useClashConnectionDetails must be used within a ClashConnectionDetailsProvider',
    )
  }

  return { data: frame, isLoading: frame === null }
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
    frame: ClashConnectionDetails_Serialize | null
  } | null>(null)

  if (frozen && captured === null) {
    setCaptured({ frame: live })
  } else if (!frozen && captured !== null) {
    setCaptured(null)
  }

  return (
    // The render that starts freezing still sees `captured === null` (the
    // state update above only applies on the re-render), so it provides
    // `live`, which is the value being captured.
    <FrameContext.Provider value={frozen && captured ? captured.frame : live}>
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
  const [subscriberCount, setSubscriberCount] = useState(0)
  const [frame, setFrame] = useState<ClashConnectionDetails_Serialize | null>(
    null,
  )

  const register = useCallback<Registrar>(() => {
    setSubscriberCount((count) => count + 1)
    return () => setSubscriberCount((count) => count - 1)
  }, [])

  // The Rust side clears its own watch to `None` on disconnect, but the
  // forwarder only ever forwards `Some` frames, so a webview learns of a
  // disconnect only by watching the connector state itself, not the channel.
  useEffect(() => {
    if (connectorState !== 'connected') setFrame(null)
  }, [connectorState])

  const hasSubscribers = subscriberCount > 0

  useEffect(() => {
    if (!hasSubscribers) return

    if (!isTauri()) {
      const source = new EventSource('/bridge/connection-details')
      source.onmessage = (event) => {
        try {
          setFrame(JSON.parse(event.data) as ClashConnectionDetails_Serialize)
        } catch (error) {
          console.error('failed to decode connection details:', error)
        }
      }
      return () => {
        source.close()
        setFrame(null)
      }
    }

    let disposed = false
    let subscriptionId: SubscriptionId | undefined

    const channel = new Channel<ClashConnectionDetails_Serialize>()
    channel.onmessage = (data) => {
      if (!disposed) setFrame(data)
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
        if (result.status === 'error') {
          console.error(
            'failed to subscribe to connection details:',
            result.error,
          )
          return
        }
        if (disposed) {
          // The last consumer unmounted before this resolved.
          unsubscribe(result.data)
          return
        }
        subscriptionId = result.data
      })
      .catch((error: unknown) => {
        console.error('failed to subscribe to connection details:', error)
      })

    return () => {
      disposed = true
      setFrame(null)
      if (subscriptionId !== undefined) {
        unsubscribe(subscriptionId)
      }
    }
  }, [hasSubscribers])

  return (
    <RegistrarContext.Provider value={register}>
      <FrameContext.Provider value={frame}>{children}</FrameContext.Provider>
    </RegistrarContext.Provider>
  )
}
