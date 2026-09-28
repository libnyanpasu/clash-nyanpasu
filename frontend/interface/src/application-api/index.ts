import { Channel } from '@tauri-apps/api/core'
import type { ApplicationApiStreams, ApplicationApiUnary } from './generated'

export type * from './generated'
export { applicationApiProcedureNames } from './generated'

type UnaryName = keyof ApplicationApiUnary & string
type StreamName = keyof ApplicationApiStreams & string

export interface ApplicationApiClient {
  call<Name extends UnaryName>(
    fnName: Name,
    params: ApplicationApiUnary[Name]['params'],
  ): Promise<ApplicationApiUnary[Name]['result']>
  subscribe<Name extends StreamName>(
    fnName: Name,
    params: ApplicationApiStreams[Name]['params'],
    onEvent: (event: ApplicationApiStreams[Name]['event']) => void,
  ): Promise<() => Promise<void>>
}

declare global {
  interface Window {
    __NYANPASU_API__?: {
      call: (fnName: string, params: unknown) => Promise<unknown>
      subscribe: (
        fnName: string,
        params: unknown,
        channel: Channel<unknown>,
      ) => Promise<number>
      unsubscribe: (subscriptionId: number) => Promise<boolean>
    }
  }
}

export const createTauriApplicationApi = (): ApplicationApiClient => {
  const bridge = window.__NYANPASU_API__
  if (!bridge) throw new Error('Application API Tauri plugin is unavailable')

  return {
    call: (fnName, params) => bridge.call(fnName, params) as Promise<never>,
    subscribe: async (fnName, params, onEvent) => {
      const channel = new Channel<unknown>()
      channel.onmessage = (event) => onEvent(event as never)
      let subscriptionId: number
      try {
        subscriptionId = await bridge.subscribe(fnName, params, channel)
      } catch (error) {
        channel.onmessage = () => undefined
        throw error
      }

      let closing: Promise<void> | undefined
      return async () => {
        if (!closing) {
          closing = bridge.unsubscribe(subscriptionId).then(() => {
            channel.onmessage = () => undefined
          })
        }
        await closing
      }
    },
  }
}

export const createHttpApplicationApi = (
  baseUrl: string,
): ApplicationApiClient => {
  const root = baseUrl.replace(/\/$/, '')
  return {
    call: async (fnName, params) => {
      const response = await fetch(`${root}/call`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ fn_name: fnName, params }),
      })
      if (!response.ok)
        throw new Error(`Application API HTTP ${response.status}`)
      return response.json()
    },
    subscribe: async (fnName, _params, onEvent) => {
      const events = new EventSource(
        `${root}/events/${encodeURIComponent(fnName)}`,
      )
      events.onmessage = (event) => onEvent(JSON.parse(event.data) as never)
      return async () => events.close()
    },
  }
}
