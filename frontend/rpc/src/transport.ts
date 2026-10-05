import { isTauri } from '@tauri-apps/api/core'
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
  return {
    commands: createCommandTransport(),
    events: isTauri()
      ? createTauriEventTransport()
      : createHttpEventTransport(),
  }
}
