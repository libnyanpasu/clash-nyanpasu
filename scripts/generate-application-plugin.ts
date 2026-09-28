import { dirname, fromFileUrl, join } from "jsr:@std/path";

const root = dirname(dirname(fromFileUrl(import.meta.url)));
const output = join(root, "backend/tauri/gen/application-api.js");
const check = Deno.args.includes("--check");
if (Deno.args.some((arg) => arg !== "--check")) {
  throw new Error("Expected only --check");
}

const temp = await Deno.makeTempDir({ prefix: "nyanpasu-plugin-" });
try {
  const compiler = new Deno.Command("pnpm", {
    cwd: root,
    args: [
      "exec",
      "tsc",
      "--ignoreConfig",
      "--target",
      "ES2022",
      "--module",
      "esnext",
      "--skipLibCheck",
      "--outDir",
      temp,
      "scripts/application-plugin-init.ts",
    ],
    stdout: "inherit",
    stderr: "inherit",
  });
  if (!(await compiler.output()).success) {
    throw new Error("Failed to compile the application API plugin script");
  }
  const generated = await Deno.readTextFile(
    join(temp, "application-plugin-init.js"),
  );
  const current = await Deno.readTextFile(output).catch(() => "");
  if (check) {
    if (current !== generated) {
      throw new Error(
        "Application API plugin script is stale; run pnpm generate:application-api",
      );
    }
  } else if (current !== generated) {
    await Deno.mkdir(dirname(output), { recursive: true });
    await Deno.writeTextFile(output, generated);
  }
} finally {
  await Deno.remove(temp, { recursive: true });
}
