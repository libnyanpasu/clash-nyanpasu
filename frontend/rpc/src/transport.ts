import { detectRuntime, type RuntimeInfo } from '@nyanpasu/platform'
import {
  createCommandTransport,
  type RpcCommandTransport,
} from './command-transport'
import {
  createHttpEventTransport,
  type RpcEventTransport,
} from './event-transport'
import { createIconTransport, type RpcIconTransport } from './icon-transport'
import { createTauriEventTransport } from './tauri-event-transport'

export type RpcTransport = {
  commands: RpcCommandTransport
  events: RpcEventTransport
  icons: RpcIconTransport
}

/** Creates fresh adapters; event connection state belongs to this result. */
export function createDefaultRpcTransport(
  runtime: RuntimeInfo = detectRuntime(),
): RpcTransport {
  return {
    commands: createCommandTransport(runtime.bridge, runtime.platform.surface),
    icons: createIconTransport(runtime.bridge === 'tauri'),
    events:
      runtime.bridge === 'tauri'
        ? createTauriEventTransport()
        : runtime.platform.surface === 'browser'
          ? createHttpEventTransport()
          : unavailableEventTransport(),
  }
}

function unavailableEventTransport(): RpcEventTransport {
  const unavailable = async (): Promise<never> => {
    throw new Error('No RPC event transport is available for this runtime')
  }
  return {
    listen: unavailable,
    once: unavailable,
    emit: unavailable,
    listenResync: () => () => {},
    dispose: () => {},
  }
}
