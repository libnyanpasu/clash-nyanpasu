import { assertEquals, assertThrows } from "jsr:@std/assert@1";
import * as path from "jsr:@std/path";
import {
  debVersion,
  releaseFileName,
  setControlVersion,
  setDebVersion,
} from "./finalize-linux-release.ts";

Deno.test("control Version is replaced in place", () => {
  assertEquals(
    setControlVersion(
      "Package: clash-nyanpasu\nVersion: 2.0.0\nArchitecture: amd64\n",
      "2.0.0~beta.1",
    ),
    "Package: clash-nyanpasu\nVersion: 2.0.0~beta.1\nArchitecture: amd64\n",
  );
  assertThrows(() => setControlVersion("Package: x\n", "2.0.0"));
});

Deno.test("bundle names carry the release version", () => {
  assertEquals(
    releaseFileName("Clash Nyanpasu_2.0.0_amd64.deb", "2.0.0", "2.0.0-beta.1"),
    "Clash Nyanpasu_2.0.0-beta.1_amd64.deb",
  );
  assertEquals(
    releaseFileName(
      "Clash Nyanpasu_2.0.0_amd64.AppImage.tar.gz.sig",
      "2.0.0",
      "2.0.0-beta.1",
    ),
    "Clash Nyanpasu_2.0.0-beta.1_amd64.AppImage.tar.gz.sig",
  );
  assertThrows(() =>
    releaseFileName("Clash Nyanpasu_1.9.0_amd64.deb", "2.0.0", "2.0.0-beta.1")
  );
});

async function hasCommand(command: string) {
  try {
    return (await new Deno.Command(command, { args: ["--version"] }).output())
      .success;
  } catch {
    return false;
  }
}
const hasDpkg = await hasCommand("dpkg-deb");

async function member(deb: string, prefix: string) {
  const list = await new Deno.Command("ar", {
    args: ["t", deb],
    stdout: "piped",
  })
    .output();
  const names = new TextDecoder().decode(list.stdout).split("\n");
  const name = names.find((entry) => entry.startsWith(prefix))!;
  const bytes = await new Deno.Command("ar", {
    args: ["p", deb, name],
    stdout: "piped",
  }).output();
  return { names: names.filter(Boolean), bytes: bytes.stdout };
}

Deno.test({
  name: "deb Version is rewritten without touching the payload",
  ignore: !hasDpkg,
  async fn() {
    const work = await Deno.makeTempDir({ prefix: "nyanpasu-deb-test-" });
    try {
      const root = path.join(work, "pkg");
      await Deno.mkdir(path.join(root, "DEBIAN"), { recursive: true });
      await Deno.mkdir(path.join(root, "usr/bin"), { recursive: true });
      await Deno.writeTextFile(path.join(root, "usr/bin/app"), "payload\n");
      await Deno.writeTextFile(
        path.join(root, "DEBIAN/control"),
        "Package: clash-nyanpasu\nVersion: 2.0.0\nArchitecture: amd64\nMaintainer: test\nDescription: test\n",
      );
      const deb = path.join(work, "Clash Nyanpasu_2.0.0_amd64.deb");
      const built = await new Deno.Command("dpkg-deb", {
        // Tauri writes gzip members; mirror it rather than dpkg's default.
        args: ["--root-owner-group", "-Zgzip", "--build", root, deb],
        stdout: "null",
      }).output();
      assertEquals(built.success, true);
      const before = await member(deb, "data.tar");

      await setDebVersion(deb, "2.0.0~beta.1");

      assertEquals(await debVersion(deb), "2.0.0~beta.1");
      const after = await member(deb, "data.tar");
      assertEquals(after.names, before.names);
      assertEquals(after.bytes, before.bytes);
      const check = await new Deno.Command("dpkg-deb", {
        args: ["--contents", deb],
        stdout: "null",
      }).output();
      assertEquals(check.success, true);
    } finally {
      await Deno.remove(work, { recursive: true });
    }
  },
});
