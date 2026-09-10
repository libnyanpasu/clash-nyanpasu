import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { stripTypeScriptTypes } from "node:module";
import test from "node:test";
import { SourceTextModule, SyntheticModule } from "node:vm";

const source = stripTypeScriptTypes(
  await readFile(
    new URL(
      "../frontend/interface/src/ipc/use-clash-proxies.ts",
      import.meta.url,
    ),
    "utf8",
  ),
);

async function setup() {
  const node = (name) => ({
    name,
    history: [{ time: "2026-09-10T00:00:00Z", delay: 42 }],
  });
  const original = {
    global: { all: [node("tested"), node("other")] },
    groups: [{ all: [node("tested"), node("other")] }],
  };
  let cached = original;
  const dependencies = {
    "@tanstack/react-query": {
      useQuery: () => ({}),
      useMutation: (options) => options,
      useQueryClient: () => ({
        getQueryData: () => cached,
        setQueryData: (_, data) => {
          cached = data;
        },
      }),
    },
    "../utils": { unwrapResult: (value) => value },
    "./bindings": { commands: {} },
    "./consts": { CLASH_PROXIES_QUERY_KEY: "clash-proxies" },
  };
  const module = new SourceTextModule(source);
  await module.link((specifier) => {
    const exports = dependencies[specifier];
    return new SyntheticModule(Object.keys(exports), function () {
      for (const [name, value] of Object.entries(exports)) {
        this.setExport(name, value);
      }
    });
  });
  await module.evaluate();
  return {
    hook: module.namespace.useClashProxies(),
    original,
    data: () => cached,
  };
}

test("group delay preserves history of nodes absent from the response", async () => {
  const { hook, original, data } = await setup();
  hook.updateGroupDelay.onSuccess({ tested: 100 });
  for (const group of [data().global, ...data().groups]) {
    assert.deepEqual(group.all[0].history.map(({ delay }) => delay), [42, 100]);
    assert.deepEqual(group.all[1].history, original.global.all[1].history);
  }
  assert.equal(original.global.all[0].history.length, 1);
});

test("empty group responses retain all existing history", async () => {
  const { hook, original, data } = await setup();
  hook.updateGroupDelay.onSuccess({});
  assert.deepEqual(data(), original);
});

test("zero delay is retained as a failed sample", async () => {
  const { hook, data } = await setup();
  hook.updateGroupDelay.onSuccess({ tested: 0 });
  assert.deepEqual(data().groups[0].all[0].history.map(({ delay }) => delay), [
    42,
    0,
  ]);
});
