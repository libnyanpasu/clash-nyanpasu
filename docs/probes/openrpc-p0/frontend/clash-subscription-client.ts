import { Channel } from "@tauri-apps/api/core";

export interface ClashEvent {
  sequence: number;
  update: string;
}

export interface TauriRpcBridge {
  dispatch(request: string): Promise<string>;
  subscribe(request: string, notifications: Channel<string>): Promise<string>;
}

interface JsonRpcResponse {
  jsonrpc: "2.0";
  id: number;
  result?: unknown;
  error?: { code: number; message: string };
}

interface ClashNotification {
  jsonrpc: "2.0";
  method: "nyanpasu.v1.clash.event";
  params: {
    subscription: number | string;
    result: ClashEvent;
  };
}

function parseResponse(raw: string, expectedId: number): JsonRpcResponse {
  const response = JSON.parse(raw) as JsonRpcResponse;
  if (response.jsonrpc !== "2.0" || response.id !== expectedId) {
    throw new Error("Unexpected JSON-RPC response envelope");
  }
  if (response.error) {
    throw new Error(`JSON-RPC ${response.error.code}: ${response.error.message}`);
  }
  return response;
}

export function createClashSubscriptionClient(bridge: TauriRpcBridge) {
  let nextRequestId = 1;

  return {
    async subscribe(onEvent: (event: ClashEvent) => void): Promise<() => Promise<void>> {
      const requestId = nextRequestId++;
      let subscriptionId: number | string | undefined;
      const channel = new Channel<string>((raw) => {
        const notification = JSON.parse(raw) as ClashNotification;
        if (
          notification.jsonrpc !== "2.0" ||
          notification.method !== "nyanpasu.v1.clash.event"
        ) {
          throw new Error("Unexpected JSON-RPC subscription notification");
        }
        if (subscriptionId !== undefined && notification.params.subscription !== subscriptionId) {
          throw new Error("Notification belongs to a different subscription");
        }
        onEvent(notification.params.result);
      });

      const subscribeResponse = parseResponse(
        await bridge.subscribe(
          JSON.stringify({
            jsonrpc: "2.0",
            id: requestId,
            method: "nyanpasu.v1.clash.subscribe",
            params: [],
          }),
          channel,
        ),
        requestId,
      );
      if (typeof subscribeResponse.result !== "number" && typeof subscribeResponse.result !== "string") {
        throw new Error("JSON-RPC subscription response has no subscription id");
      }
      subscriptionId = subscribeResponse.result;

      let closed = false;
      return async () => {
        if (closed) return;
        closed = true;

        const unsubscribeId = nextRequestId++;
        const unsubscribeResponse = parseResponse(
          await bridge.dispatch(
            JSON.stringify({
              jsonrpc: "2.0",
              id: unsubscribeId,
              method: "nyanpasu.v1.clash.unsubscribe",
              params: [subscriptionId],
            }),
          ),
          unsubscribeId,
        );
        if (unsubscribeResponse.result !== true) {
          throw new Error("JSON-RPC subscription was not active");
        }
      };
    },
  };
}
