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

/** Cheapest authenticated call, used to learn why a stream closed. */
const RPC_DESCRIBE_COMMAND = 'rpc_describe'

export type RpcTransport = {
  commands: RpcCommandTransport
  events: RpcEventTransport
  icons: RpcIconTransport
}

export interface DefaultRpcTransportOptions {
  /** Called on an HTTP `401`, including one found by probing a closed stream. */
  onUnauthorized?: () => void
}

/** Creates fresh adapters; event connection state belongs to this result. */
export function createDefaultRpcTransport(
  runtime: RuntimeInfo = detectRuntime(),
  options: DefaultRpcTransportOptions = {},
): RpcTransport {
  const commands = createCommandTransport(
    runtime.bridge,
    runtime.platform.surface,
    options,
  )

  return {
    commands,
    icons: createIconTransport(runtime.bridge === 'tauri'),
    events:
      runtime.bridge === 'tauri'
        ? createTauriEventTransport()
        : runtime.platform.surface === 'browser'
          ? createHttpEventTransport({
              probeAuth: () => commands.invoke(RPC_DESCRIBE_COMMAND),
            })
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
