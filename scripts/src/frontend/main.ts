import { WORKSPACE_ROOT } from "../shared/repo-paths.ts";
import { checkFrontendBoundaries } from "./policy.ts";
import { scanFrontendPackages } from "./scan.ts";

/** Run the frontend package-boundary gate. */
export async function main(): Promise<number> {
  const packages = await scanFrontendPackages(WORKSPACE_ROOT);
  const issues = checkFrontendBoundaries(packages, WORKSPACE_ROOT);

  if (issues.length > 0) {
    console.error("Frontend package boundary violations:");
    for (const issue of issues) console.error(`- ${issue}`);
    return 1;
  }

  const sharedCount = packages.filter((pkg) => pkg.role === "shared").length;
  console.log(
    `Frontend package boundaries passed (${sharedCount} shared packages).`,
  );
  return 0;
}

if (import.meta.main) {
  main().then((status) => Deno.exit(status)).catch((error) => {
    console.error(error);
    Deno.exit(1);
  });
}
