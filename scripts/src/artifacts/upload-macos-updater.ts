import { WORKSPACE_ROOT } from "../shared/repo-paths.ts";
import * as path from "node:path";
import { globby } from "globby";
import { consola } from "../shared/logger.ts";
import { rename } from "node:fs/promises";

consola.info(`WORKSPACE_ROOT: ${WORKSPACE_ROOT}`);

const TARGET_ARCH = process.env["TARGET_ARCH"] ||
  (process.arch === "arm64" ? "aarch64" : "x86_64");

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
    await rename(file, newPath);
  }
}

consola.success("Files renamed successfully");
