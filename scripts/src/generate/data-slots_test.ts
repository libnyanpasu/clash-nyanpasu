import assert from "node:assert/strict";
import { extractDataSlots } from "./data-slots.ts";

Deno.test("slot inventory preserves both shared log viewer identities", () => {
  assert.deepEqual(
    extractDataSlots(`
    <div data-slot="logs-virtual-list" />
    <div data-slot={displaySource === 'core' ? 'core-logs' : 'file-logs'} />
    <div data-slot={
      displaySource === 'core'
        ? 'core-logs-viewer'
        : 'file-logs-viewer'
    } />
  `),
    [
      "logs-virtual-list",
      "core-logs",
      "file-logs",
      "core-logs-viewer",
      "file-logs-viewer",
    ],
  );
});

Deno.test("slot inventory deduplicates literals without treating other props as slots", () => {
  assert.deepEqual(
    extractDataSlots(`
    <div data-slot='panel' />
    <div data-slot="panel" />
    <div title={ready ? 'ready' : 'pending'} data-slot={slot} />
    <div data-slot={ready ? "panel" : "loading"} />
  `),
    ["panel", "loading"],
  );
});
