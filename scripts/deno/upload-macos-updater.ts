import * as path from "jsr:@std/path";
import { globby } from "npm:globby";
import { consola } from "./utils/logger.ts";

const WORKSPACE_ROOT = path.join(import.meta.dirname!, "../..");
consola.info(`WORKSPACE_ROOT: ${WORKSPACE_ROOT}`);

const TARGET_ARCH = Deno.env.get("TARGET_ARCH") || Deno.build.arch;

const BACKEND_BUILD_DIR = path.join(WORKSPACE_ROOT, "backend/target");

const files = await globby(["**/*.tar.gz", "**/*.sig", "**/*.dmg"], {
  cwd: BACKEND_BUILD_DIR,
});

for (let file of files) {
  file = path.join(BACKEND_BUILD_DIR, file);
  const p = path.parse(file);
  consola.info(`Found file: ${p.base}`);
  if (p.base.includes(".app")) {
    const components = p.base.split(".");
    const newName = components[0] +
      `.${TARGET_ARCH}.${components.slice(1).join(".")}`;
    const newPath = path.join(p.dir, newName);
    consola.info(`Renaming ${file} to ${newPath}`);
    await Deno.rename(file, newPath);
  }
}

consola.success("Files renamed successfully");
