import assert from "node:assert/strict";
import { test } from "node:test";
import { formatBytes } from "./format-bytes.ts";

test("formatBytes uses decimal units with three significant digits", () => {
  assert.deepStrictEqual(formatBytes(0), "0 B");
  assert.deepStrictEqual(formatBytes(999), "999 B");
  assert.deepStrictEqual(formatBytes(1000), "1 kB");
  assert.deepStrictEqual(formatBytes(1536), "1.54 kB");
  assert.deepStrictEqual(formatBytes(5_242_880), "5.24 MB");
  assert.deepStrictEqual(formatBytes(123_456_789_012), "123 GB");
  assert.deepStrictEqual(formatBytes(-2048), "-2.05 kB");
});

test("formatBytes rejects non-finite counts", () => {
  assert.throws(() => formatBytes(Number.NaN), TypeError);
});
