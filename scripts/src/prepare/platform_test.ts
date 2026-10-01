import { assertEquals, assertThrows } from "jsr:@std/assert@1";
import { mapArch, normalizeArch, normalizePlatform } from "./platform.ts";

Deno.test("normalizes Deno platform and architecture names", () => {
  assertEquals(normalizePlatform("windows"), "win32");
  assertEquals(normalizePlatform("darwin"), "darwin");
  assertEquals(normalizeArch("x86_64"), "x64");
  assertEquals(normalizeArch("aarch64"), "arm64");
  assertEquals(normalizeArch("ia32"), "ia32");
});

Deno.test("maps supported platform and architecture pairs", () => {
  assertEquals(mapArch("darwin", "arm64"), "darwin-arm64");
  assertEquals(mapArch("win32", "x64"), "windows-x86_64");
  assertEquals(mapArch("linux", "arm64"), "linux-aarch64");
});

Deno.test("rejects unsupported platform and architecture pairs", () => {
  assertThrows(
    () => mapArch("freebsd", "x64"),
    Error,
    "Unsupported platform/architecture: freebsd-x64",
  );
});
