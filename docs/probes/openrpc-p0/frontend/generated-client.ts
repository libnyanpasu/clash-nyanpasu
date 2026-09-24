import NyanpasuApplicationAPIProbe from "./generated/client/typescript/src/index.ts";

import { TauriInvokeTransport, type InvokeRpc } from "./tauri-invoke-transport.ts";

export function createOpenRpcClient(invokeRpc: InvokeRpc) {
  const generated = new NyanpasuApplicationAPIProbe({
    transport: { type: "http", host: "127.0.0.1", port: 1 },
  });
  generated.rpc.requestManager.transports = [new TauriInvokeTransport(invokeRpc)];

  return {
    profiles: {
      list: () => generated["nyanpasu.v1.profiles.list"](),
      activate: (profileId: string) => generated["nyanpasu.v1.profiles.activate"](profileId),
    },
    discover: () => generated["rpc.discover"](),
  };
}
