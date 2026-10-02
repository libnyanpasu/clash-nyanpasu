import * as path from "jsr:@std/path";
import type { FrontendPackage, FrontendPackageRole } from "./policy.ts";

const sourceExtensions = new Set([
  ".ts",
  ".tsx",
  ".mts",
  ".cts",
  ".js",
  ".jsx",
]);

function extractImports(source: string): string[] {
  const imports = new Set<string>();
  const patterns = [
    /\bimport\s*["']([^"']+)["']/g,
    /\b(?:import|export)\s+(?:type\s+)?[\s\S]*?\sfrom\s*["']([^"']+)["']/g,
    /\bimport\s*\(\s*["']([^"']+)["']\s*\)/g,
  ];

  for (const pattern of patterns) {
    for (const match of source.matchAll(pattern)) {
      imports.add(match[1]);
    }
  }
  return [...imports];
}

async function sourceFiles(directory: string): Promise<string[]> {
  const files: string[] = [];
  try {
    for await (const entry of Deno.readDir(directory)) {
      const entryPath = path.join(directory, entry.name);
      if (entry.isDirectory) {
        files.push(...await sourceFiles(entryPath));
      } else if (
        entry.isFile && sourceExtensions.has(path.extname(entry.name))
      ) {
        files.push(entryPath);
      }
    }
  } catch (error) {
    if (!(error instanceof Deno.errors.NotFound)) throw error;
  }
  return files.sort();
}

function roleFor(
  pathFromRoot: string,
): FrontendPackageRole {
  if (pathFromRoot === "frontend/nyanpasu") return "app";
  return "shared";
}

/** Read package manifests and production source imports under frontend/*. */
export async function scanFrontendPackages(
  workspaceRoot: string,
): Promise<FrontendPackage[]> {
  const frontendRoot = path.join(workspaceRoot, "frontend");
  const packages: FrontendPackage[] = [];

  for await (const entry of Deno.readDir(frontendRoot)) {
    if (!entry.isDirectory) continue;
    const packagePath = path.join(frontendRoot, entry.name);
    const manifestPath = path.join(packagePath, "package.json");
    let manifest: Record<string, unknown>;
    try {
      manifest = JSON.parse(await Deno.readTextFile(manifestPath));
    } catch (error) {
      if (error instanceof Deno.errors.NotFound) continue;
      throw new Error(`failed to read ${manifestPath}: ${String(error)}`, {
        cause: error,
      });
    }

    const packageName = String(manifest.name ?? entry.name);
    const relativePackagePath = `frontend/${entry.name}`;
    const sources: FrontendPackage["sources"] = [];
    for (
      const absoluteSourcePath of await sourceFiles(
        path.join(packagePath, "src"),
      )
    ) {
      const relativeSourcePath = path.relative(
        workspaceRoot,
        absoluteSourcePath,
      );
      const source = await Deno.readTextFile(absoluteSourcePath);
      sources.push({
        path: relativeSourcePath,
        imports: extractImports(source),
        source,
      });
    }

    const packageRecord: FrontendPackage = {
      name: packageName,
      path: relativePackagePath,
      role: roleFor(relativePackagePath),
      private: manifest.private === true,
      exports: manifest.exports,
      main: typeof manifest.main === "string" ? manifest.main : undefined,
      types: typeof manifest.types === "string" ? manifest.types : undefined,
      dependencies: dependencyRecord(manifest.dependencies),
      devDependencies: dependencyRecord(manifest.devDependencies),
      peerDependencies: dependencyRecord(manifest.peerDependencies),
      optionalDependencies: dependencyRecord(manifest.optionalDependencies),
      sources,
    };
    packages.push(packageRecord);
  }

  return packages.sort((a, b) => a.name.localeCompare(b.name));
}

function dependencyRecord(value: unknown): Record<string, string> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return {};
  }
  return Object.fromEntries(
    Object.entries(value).filter((entry): entry is [string, string] =>
      typeof entry[1] === "string"
    ),
  );
}
