import { invoke, type Channel } from "@tauri-apps/api/core";

import type { TauriRpcBridge } from "./clash-subscription-client.ts";

export type TauriInvoke = <T>(
  command: string,
  args: Record<string, unknown>,
) => Promise<T>;

export function createTauriRpcBridge(invokeRpc: TauriInvoke = invoke): TauriRpcBridge {
  return {
    dispatch: (request) => invokeRpc<string>("rpc_dispatch", { request }),
    subscribe: (request, notifications: Channel<string>) =>
      invokeRpc<string>("rpc_subscribe", { request, notifications }),
  };
}
