import assert from "node:assert/strict";

import {
  createApiFetch,
  type ApiRequest,
  type ApiResponse,
} from "./api-transport.ts";

const requests: ApiRequest[] = [];
const apiFetch = createApiFetch(async (request) => {
  requests.push(request);

  const response: ApiResponse = request.method === "POST"
    ? {
      status: 200,
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ profile_id: "profile-2", active: true }),
    }
    : {
      status: 200,
      headers: { "content-type": "application/json" },
      body: JSON.stringify([{ id: "profile-2", name: "Backup", active: false }]),
    };

  return response;
});

const profiles = await apiFetch<{
  data: Array<{ id: string; name: string; active: boolean }>;
  status: number;
  headers: Headers;
}>("/api/v1/profiles?limit=20", { method: "GET" });

assert.equal(profiles.status, 200);
assert.equal(profiles.data[0]?.id, "profile-2");
assert.equal(requests[0]?.uri, "/api/v1/profiles?limit=20");

const activation = await apiFetch<{
  data: { profile_id: string; active: boolean };
  status: number;
  headers: Headers;
}>("/api/v1/profiles/active", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ profile_id: "profile-2" }),
});

assert.equal(activation.data.active, true);
assert.equal(requests[1]?.body, JSON.stringify({ profile_id: "profile-2" }));
assert.equal(requests[1]?.headers["content-type"], "application/json");

const errorApiFetch = createApiFetch(async () => ({
  status: 404,
  headers: { "content-type": "application/json" },
  body: JSON.stringify({ code: "profile_not_found", message: "missing" }),
}));

let thrown: unknown;
try {
  await errorApiFetch("/api/v1/profiles/active", { method: "POST" });
} catch (error) {
  thrown = error;
}

assert.deepEqual(thrown, { code: "profile_not_found", message: "missing" });

console.log("API transport adapter smoke checks passed");
