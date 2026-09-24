import assert from "node:assert/strict";
import type { Channel } from "@tauri-apps/api/core";

import { createClashSubscriptionClient } from "./clash-subscription-client.ts";
import { createOpenRpcClient } from "./generated-client.ts";
import { createTauriRpcBridge, type TauriInvoke } from "./tauri-rpc-bridge.ts";

const wireRequests: Array<Record<string, unknown>> = [];
const client = createOpenRpcClient(async (requestJson) => {
  const request = JSON.parse(requestJson) as Record<string, unknown>;
  wireRequests.push(request);

  const id = request.id;
  if (request.method === "nyanpasu.v1.profiles.list") {
    return JSON.stringify({
      jsonrpc: "2.0",
      id,
      result: [{ id: "profile-1", name: "Primary", active: true }],
    });
  }
  if (request.method === "nyanpasu.v1.profiles.activate") {
    const params = request.params as unknown[];
    if (params[0] === "missing") {
      return JSON.stringify({
        jsonrpc: "2.0",
        id,
        error: { code: -32004, message: "Profile not found", data: { profileId: "missing" } },
      });
    }
    return JSON.stringify({
      jsonrpc: "2.0",
      id,
      result: { profileId: params[0], active: true },
    });
  }
  return JSON.stringify({ jsonrpc: "2.0", id, result: { openrpc: "1.2.6" } });
});

assert.deepEqual(await client.profiles.list(), [
  { id: "profile-1", name: "Primary", active: true },
]);
assert.deepEqual(await client.profiles.activate("profile-2"), {
  profileId: "profile-2",
  active: true,
});
assert.deepEqual(await client.discover(), { openrpc: "1.2.6" });
assert.deepEqual(wireRequests[1]?.params, ["profile-2"]);

await assert.rejects(
  () => client.profiles.activate("missing"),
  (error: Error & { code?: number; data?: unknown }) =>
    error.code === -32004 &&
    JSON.stringify(error.data) === JSON.stringify({ profileId: "missing" }),
);

const channelCallbacks = new Map<number, (payload: unknown) => void>();
let nextChannelId = 1;
Reflect.set(globalThis, "window", {
  __TAURI_INTERNALS__: {
    transformCallback(callback: (payload: unknown) => void) {
      const id = nextChannelId++;
      channelCallbacks.set(id, callback);
      return id;
    },
    unregisterCallback(id: number) {
      channelCallbacks.delete(id);
    },
  },
});

const subscriptionRequests: Array<Record<string, unknown>> = [];
let subscriptionChannelId: number | undefined;
let unsubscribeCalls = 0;
const invokeMock: TauriInvoke = async <T>(command: string, args: Record<string, unknown>) => {
  const request = JSON.parse(args.request as string) as Record<string, unknown>;
  subscriptionRequests.push(request);

  if (command === "rpc_subscribe") {
    assert.equal(request.method, "nyanpasu.v1.clash.subscribe");
    const channel = args.notifications as Channel<string>;
    subscriptionChannelId = channel.id;
    assert.equal(JSON.parse(JSON.stringify(channel)), `__CHANNEL__:${channel.id}`);

    const callback = channelCallbacks.get(channel.id);
    assert.ok(callback);
    callback({
      index: 1,
      message: JSON.stringify({
        jsonrpc: "2.0",
        method: "nyanpasu.v1.clash.event",
        params: {
          subscription: 42,
          result: { sequence: 2, update: "second" },
        },
      }),
    });
    callback({
      index: 0,
      message: JSON.stringify({
        jsonrpc: "2.0",
        method: "nyanpasu.v1.clash.event",
        params: {
          subscription: 42,
          result: { sequence: 1, update: "first" },
        },
      }),
    });

    return JSON.stringify({ jsonrpc: "2.0", id: request.id, result: 42 }) as T;
  }

  assert.equal(command, "rpc_dispatch");
  assert.equal(request.method, "nyanpasu.v1.clash.unsubscribe");
  unsubscribeCalls++;
  const channelCallback = channelCallbacks.get(subscriptionChannelId!);
  assert.ok(channelCallback);
  channelCallback({ index: 2, end: true });
  return JSON.stringify({ jsonrpc: "2.0", id: request.id, result: true }) as T;
};

const clashEvents = createClashSubscriptionClient(createTauriRpcBridge(invokeMock));
const receivedEvents: Array<{ sequence: number; update: string }> = [];
const stopClashEvents = await clashEvents.subscribe((event) => receivedEvents.push(event));
assert.deepEqual(receivedEvents, [
  { sequence: 1, update: "first" },
  { sequence: 2, update: "second" },
]);
await stopClashEvents();
await stopClashEvents();
assert.equal(unsubscribeCalls, 1);
assert.equal(channelCallbacks.has(subscriptionChannelId!), false);
assert.deepEqual(
  subscriptionRequests.map((request) => request.method),
  ["nyanpasu.v1.clash.subscribe", "nyanpasu.v1.clash.unsubscribe"],
);

console.log("OpenRPC unary and Tauri Channel subscription smoke passed");
