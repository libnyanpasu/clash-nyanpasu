import * as path from "jsr:@std/path";
import { globby } from "npm:globby";
import { consola } from "../shared/logger.ts";

// create-dmg puts .background beyond the window's right edge. Finder includes
// that hidden item's saved location when calculating its horizontal scroll range.
export function containBackgroundIcon(bytes: Uint8Array): Uint8Array {
  const marker = new Uint8Array([
    0,
    0,
    0,
    11,
    ...Array.from(".background").flatMap((char) => [0, char.charCodeAt(0)]),
    ...new TextEncoder().encode("Ilocblob"),
    0,
    0,
    0,
    16,
  ]);
  const result = bytes.slice();
  const view = new DataView(result.buffer);
  let found = false;
  for (let i = 0; i <= bytes.length - marker.length - 16; i++) {
    if (!marker.every((value, offset) => bytes[i + offset] === value)) continue;
    // Keep the record and its allocation unchanged; only replace the x/y values.
    // Leave room for the 128px icon and its label even with hidden files shown.
    view.setUint32(i + marker.length, 160);
    view.setUint32(i + marker.length + 4, 100);
    found = true;
  }
  if (!found) throw new Error("Missing .background icon-location record");
  return result;
}

async function run(args: string[]) {
  const result = await new Deno.Command("hdiutil", {
    args,
    stdout: "piped",
    stderr: "piped",
  }).output();
  if (!result.success) {
    throw new Error(new TextDecoder().decode(result.stderr));
  }
}

export async function finalizeDmg(file: string) {
  const work = await Deno.makeTempDir({
    dir: path.dirname(file),
    prefix: ".nyanpasu-dmg-",
  });
  const mount = path.join(work, "mount");
  const writable = path.join(work, "writable.dmg");
  const finished = path.join(work, "finished.dmg");
  let attached = false;
  try {
    await Deno.mkdir(mount);
    await run(["convert", file, "-format", "UDRW", "-o", writable]);
    await run([
      "attach",
      writable,
      "-readwrite",
      "-noautoopen",
      "-nobrowse",
      "-mountpoint",
      mount,
    ]);
    attached = true;
    const store = path.join(mount, ".DS_Store");
    await Deno.writeFile(
      store,
      containBackgroundIcon(await Deno.readFile(store)),
    );
    await run(["detach", mount]);
    attached = false;
    await run(["convert", writable, "-format", "UDZO", "-o", finished]);
    await run(["verify", finished]);
    // Never replace the original until conversion and verification succeed.
    await Deno.rename(finished, file);
    consola.success(
      `Contained hidden background icon in ${path.basename(file)}`,
    );
  } finally {
    if (attached) await run(["detach", mount]);
    await Deno.remove(work, { recursive: true });
  }
}

if (import.meta.main) {
  if (Deno.build.os !== "darwin") {
    throw new Error("DMG finalization requires macOS");
  }
  const files = Deno.args.length
    ? Deno.args.map((file) => path.resolve(file))
    : await globby("backend/target/**/bundle/dmg/*.dmg", { absolute: true });
  if (files.length === 0) throw new Error("No DMG packages found");
  for (const file of files) await finalizeDmg(file);
}
