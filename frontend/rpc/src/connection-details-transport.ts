import type { RuntimeInfo } from '@nyanpasu/platform'
import { Channel } from '@tauri-apps/api/core'
import { createClosedStreamProbe, type AuthProbe } from './auth-probe'
import { withClientId } from './client-id'

export type RpcResult<T> =
  { status: 'ok'; data: T } | { status: 'error'; error: unknown }

export interface ConnectionDetailsRpc<T> {
  subscribe(channel: Channel<T>): Promise<RpcResult<unknown>>
  unsubscribe(id: unknown): Promise<RpcResult<unknown>>
}

export interface ConnectionDetailsCallbacks<T> {
  onOpen: () => void
  onMessage: (message: T) => void
  onError: (error: unknown) => void
}

export interface ConnectionDetailsTransport<T> {
  listen(callbacks: ConnectionDetailsCallbacks<T>): () => void
}

export interface ConnectionDetailsTransportOptions {
  /** Probes credentials when the stream closes for good, to detect `401`. */
  probeAuth?: AuthProbe
}

/** Selects the dedicated connection stream from the same runtime facts as RPC. */
export function createConnectionDetailsTransport<T>(
  runtime: RuntimeInfo,
  rpc: ConnectionDetailsRpc<T>,
  options: ConnectionDetailsTransportOptions = {},
): ConnectionDetailsTransport<T> {
  const probeClosedStream = createClosedStreamProbe(options.probeAuth)

  return {
    listen({ onOpen, onMessage, onError }) {
      if (runtime.platform.surface === 'browser') {
        const source = new EventSource(
          withClientId('/bridge/connection-details'),
        )
        source.onopen = onOpen
        source.onmessage = (event) => {
          try {
            onMessage(JSON.parse(event.data) as T)
          } catch (error) {
            console.error('failed to decode connection details:', error)
            onError(error)
          }
        }
        source.onerror = (event) => {
          probeClosedStream(source)
          onError(event)
        }
        return () => source.close()
      }

      if (runtime.bridge !== 'tauri') {
        onError(new Error('No connection details transport is available'))
        return () => {}
      }

      let disposed = false
      let subscriptionId: unknown
      const channel = new Channel<T>()
      channel.onmessage = (message) => {
        if (!disposed) onMessage(message)
      }
      const unsubscribe = (id: unknown) => {
        rpc
          .unsubscribe(id)
          .then((result) => {
            if (result.status === 'error') {
              console.error(
                'failed to unsubscribe from connection details:',
                result.error,
              )
            }
          })
          .catch((error: unknown) => {
            console.error(
              'failed to unsubscribe from connection details:',
              error,
            )
          })
      }
      rpc
        .subscribe(channel)
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
            onError(result.error)
            return
          }
          subscriptionId = result.data
        })
        .catch((error: unknown) => {
          console.error('failed to subscribe to connection details:', error)
          if (!disposed) onError(error)
        })

      return () => {
        disposed = true
        if (subscriptionId !== undefined) unsubscribe(subscriptionId)
      }
    },
  }
}
