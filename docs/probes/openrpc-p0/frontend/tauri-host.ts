import { invoke } from "@tauri-apps/api/core";

import { createOpenRpcClient } from "./generated-client.ts";
import { createClashSubscriptionClient } from "./clash-subscription-client.ts";
import { createTauriRpcBridge } from "./tauri-rpc-bridge.ts";

export const openRpcClient = createOpenRpcClient((request) =>
  invoke<string>("rpc_dispatch", { request }),
);

export const tauriRpcBridge = createTauriRpcBridge();
export const clashSubscriptionClient = createClashSubscriptionClient(tauriRpcBridge);
