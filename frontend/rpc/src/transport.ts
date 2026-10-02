import {
  createCommandTransport,
  type RpcCommandTransport,
} from './command-transport'
import {
  createHttpEventTransport,
  type RpcEventTransport,
} from './event-transport'
import { createTauriEventTransport } from './tauri-event-transport'

export type RpcTransport = {
  commands: RpcCommandTransport
  events: RpcEventTransport
}

/** Creates fresh adapters; event connection state belongs to this result. */
export function createDefaultRpcTransport(): RpcTransport {
  const isTauri =
    typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

  return {
    commands: createCommandTransport(),
    events: isTauri ? createTauriEventTransport() : createHttpEventTransport(),
  }
}
