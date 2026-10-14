import { WORKSPACE_ROOT } from "../shared/repo-paths.ts";

const NEUTRAL_PACKAGES = ["nyanpasu-core", "nyanpasu-config"];

/** Inspect resolved production dependencies, including transitive GUI leaks. */
export function dependencyViolations(
  owner: string,
  cargoTree: string,
): string[] {
  const names = new Set(
    cargoTree.split("\n").map((line) => line.trim().split(/\s+/)[0]).filter(
      Boolean,
    ),
  );
  return [...names].filter((name) =>
    name === "clash-nyanpasu" || name === "nyanpasu-egui" ||
    name === "eframe" || name === "egui" || name === "gtk" ||
    name === "webkit2gtk" || name === "tauri" ||
    name.startsWith("tauri-") ||
    (owner === "nyanpasu-config" && name === "nyanpasu-core")
  ).map((name) => `${owner} must not depend on ${name}`);
}

export async function main(): Promise<number> {
  const problems: string[] = [];
  for (const owner of NEUTRAL_PACKAGES) {
    const result = await new Deno.Command("cargo", {
      cwd: WORKSPACE_ROOT,
      args: [
        "tree",
        "--locked",
        "--all-features",
        "--manifest-path",
        "backend/Cargo.toml",
        "--package",
        owner,
        "--target",
        "all",
        "--edges",
        "normal,build",
        "--prefix",
        "none",
        "--format",
        "{p}",
      ],
      stdout: "piped",
      stderr: "piped",
    }).output();
    if (!result.success) {
      console.error(new TextDecoder().decode(result.stderr));
      return result.code || 1;
    }
    problems.push(
      ...dependencyViolations(owner, new TextDecoder().decode(result.stdout)),
    );
  }
  if (problems.length > 0) {
    console.error(problems.join("\n"));
    return 1;
  }
  console.log("Backend core/config boundaries passed.");
  return 0;
}

if (import.meta.main) {
  main().then((code) => Deno.exit(code)).catch((error) => {
    console.error(error);
    Deno.exit(1);
  });
}
