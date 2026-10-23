import { assertEquals, assertThrows } from "jsr:@std/assert@1";
import { formatBytes } from "./format-bytes.ts";

Deno.test("formatBytes uses decimal units with three significant digits", () => {
  assertEquals(formatBytes(0), "0 B");
  assertEquals(formatBytes(999), "999 B");
  assertEquals(formatBytes(1000), "1 kB");
  assertEquals(formatBytes(1536), "1.54 kB");
  assertEquals(formatBytes(5_242_880), "5.24 MB");
  assertEquals(formatBytes(123_456_789_012), "123 GB");
  assertEquals(formatBytes(-2048), "-2.05 kB");
});

Deno.test("formatBytes rejects non-finite counts", () => {
  assertThrows(() => formatBytes(Number.NaN), TypeError);
});
