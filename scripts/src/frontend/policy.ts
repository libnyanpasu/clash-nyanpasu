import * as path from "jsr:@std/path";

export type FrontendPackageRole = "shared" | "app";

export type FrontendSource = {
  path: string;
  imports: string[];
  source?: string;
};

export type FrontendPackage = {
  name: string;
  path: string;
  role: FrontendPackageRole;
  private: boolean;
  exports: unknown;
  main?: string;
  types?: string;
  dependencies: Record<string, string>;
  devDependencies?: Record<string, string>;
  peerDependencies?: Record<string, string>;
  optionalDependencies?: Record<string, string>;
  sources: FrontendSource[];
};

const dependencySections = [
  "dependencies",
  "devDependencies",
  "peerDependencies",
  "optionalDependencies",
] as const;

const dependenciesOf = (pkg: FrontendPackage) =>
  Object.fromEntries(
    dependencySections.flatMap((section) =>
      Object.entries(pkg[section] ?? {}).map((
        [name, version],
      ) => [name, version])
    ),
  );

const sourceTargets = (exports: unknown): string[] => {
  if (typeof exports === "string") return [exports];
  if (Array.isArray(exports)) return exports.flatMap(sourceTargets);
  if (exports !== null && typeof exports === "object") {
    return Object.values(exports).flatMap(sourceTargets);
  }
  return [];
};

const packageNameFromSpecifier = (specifier: string) => {
  if (!specifier.startsWith("@nyanpasu/")) return null;
  const [, name] = specifier.split("/");
  return name ? `@nyanpasu/${name}` : null;
};

const appPathForSpecifier = (
  sourcePath: string,
  specifier: string,
  workspaceRoot: string,
) => {
  if (!specifier.startsWith(".")) return false;
  const cleanSpecifier = specifier.split(/[?#]/, 1)[0];
  const target = path.resolve(
    workspaceRoot,
    path.dirname(sourcePath),
    cleanSpecifier,
  );
  const appRoot = path.resolve(workspaceRoot, "frontend/nyanpasu");
  return target === appRoot || target.startsWith(`${appRoot}${path.SEPARATOR}`);
};

const matchesPackage = (pkg: FrontendPackage, name: string) =>
  pkg.name === name || Object.hasOwn(dependenciesOf(pkg), name);

function visitWorkspaceDependencies(
  name: string,
  packages: Map<string, FrontendPackage>,
  visiting: string[],
  visited: Set<string>,
  issues: Set<string>,
) {
  if (visited.has(name)) return;
  const cycleStart = visiting.indexOf(name);
  if (cycleStart >= 0) {
    const cycle = [...visiting.slice(cycleStart), name];
    issues.add(`workspace dependency cycle: ${cycle.join(" -> ")}`);
    return;
  }

  const pkg = packages.get(name);
  if (!pkg) return;
  visiting.push(name);
  for (const [dependency, value] of Object.entries(dependenciesOf(pkg))) {
    if (value.startsWith("workspace:") && packages.has(dependency)) {
      visitWorkspaceDependencies(
        dependency,
        packages,
        visiting,
        visited,
        issues,
      );
    }
  }
  visiting.pop();
  visited.add(name);
}

/** Analyze package metadata and source imports without reading the filesystem. */
export function checkFrontendBoundaries(
  inventory: FrontendPackage[],
  workspaceRoot: string,
): string[] {
  const issues = new Set<string>();
  const packages = new Map(inventory.map((pkg) => [pkg.name, pkg]));

  for (const pkg of inventory) {
    const dependencies = dependenciesOf(pkg);
    if (pkg.name === "@nyanpasu/interface") {
      issues.add("@nyanpasu/interface: legacy package must be removed");
    }

    for (const [dependency, value] of Object.entries(dependencies)) {
      if (!value.startsWith("workspace:")) continue;
      if (!packages.has(dependency)) {
        issues.add(
          `${pkg.name}: workspace dependency ${dependency} is missing`,
        );
      }
    }

    if (pkg.role !== "shared") continue;

    if (pkg.private !== true) {
      issues.add(`${pkg.name}: shared frontend package must be private`);
    }

    const targets = sourceTargets(pkg.exports);
    if (
      targets.length === 0 ||
      targets.some((target) =>
        !(target.startsWith("./src/") || target.startsWith("src/"))
      )
    ) {
      issues.add(
        `${pkg.name}: exports must point to package source under src/`,
      );
    }
    for (
      const [field, value] of [["main", pkg.main], [
        "types",
        pkg.types,
      ]] as const
    ) {
      if (value && !value.startsWith("./src/") && !value.startsWith("src/")) {
        issues.add(
          `${pkg.name}: ${field} must point to package source under src/`,
        );
      }
    }

    for (const dependency of Object.keys(dependencies)) {
      if (
        dependency === "@nyanpasu/nyanpasu" ||
        dependency === "@nyanpasu/interface"
      ) {
        issues.add(
          `${pkg.name}: shared packages cannot depend on ${dependency}`,
        );
      }
    }

    const blockedDependencies = pkg.name === "@nyanpasu/constants" ||
        pkg.name === "@nyanpasu/utils"
      ? ["react", "react-dom", "@tauri-apps/api"]
      : pkg.name === "@nyanpasu/hooks"
      ? [
        "@nyanpasu/rpc",
        "@nyanpasu/query",
        "@nyanpasu/platform",
        "@tanstack/react-query",
        "@tauri-apps/api",
      ]
      : pkg.name === "@nyanpasu/rpc"
      ? [
        "react",
        "react-dom",
        "@tanstack/react-query",
        "@nyanpasu/query",
        "@nyanpasu/hooks",
        "@nyanpasu/ui",
      ]
      : pkg.name === "@nyanpasu/ui"
      ? [
        "@nyanpasu/rpc",
        "@nyanpasu/query",
        "@nyanpasu/platform",
        "@nyanpasu/nyanpasu",
        "@nyanpasu/interface",
        "@tauri-apps/api",
      ]
      : [];

    const blockedImports =
      pkg.name === "@nyanpasu/hooks" || pkg.name === "@nyanpasu/ui"
        ? [...blockedDependencies, "@tauri-apps/"]
        : blockedDependencies;

    for (const blocked of blockedDependencies) {
      if (matchesPackage(pkg, blocked)) {
        issues.add(
          `${pkg.name}: dependency on ${blocked} crosses its package boundary`,
        );
      }
    }

    for (const source of pkg.sources) {
      for (const specifier of source.imports) {
        if (
          specifier.startsWith("@/") || specifier === "@interface" ||
          specifier.startsWith("@interface/")
        ) {
          issues.add(
            `${pkg.name}: app-only import ${specifier} in ${source.path}`,
          );
        }
        if (appPathForSpecifier(source.path, specifier, workspaceRoot)) {
          issues.add(
            `${pkg.name}: import into frontend/nyanpasu in ${source.path}`,
          );
        }

        if (
          blockedImports.some((blocked) =>
            blocked.endsWith("/")
              ? specifier.startsWith(blocked)
              : specifier === blocked || specifier.startsWith(`${blocked}/`)
          )
        ) {
          issues.add(
            `${pkg.name}: import ${specifier} crosses its package boundary`,
          );
        }

        const importedPackage = packageNameFromSpecifier(specifier);
        if (!importedPackage || importedPackage === pkg.name) continue;
        if (importedPackage === "@nyanpasu/interface") {
          issues.add(`${pkg.name}: import from legacy @nyanpasu/interface`);
        }
        if (importedPackage === "@nyanpasu/nyanpasu") {
          issues.add(`${pkg.name}: import from the application package`);
        }
        if (
          !dependencies[importedPackage]?.startsWith("workspace:")
        ) {
          issues.add(
            `${pkg.name}: import ${importedPackage} without a workspace dependency`,
          );
        }
      }

      if (
        (pkg.name === "@nyanpasu/constants" ||
          pkg.name === "@nyanpasu/utils") &&
        source.source &&
        /\b(?:window|document|navigator|localStorage|HTMLElement|MouseEvent|ResizeObserver)\b/
          .test(
            source.source,
          )
      ) {
        issues.add(`${pkg.name}: DOM access in ${source.path}`);
      }

      if (
        (pkg.name === "@nyanpasu/constants" ||
          pkg.name === "@nyanpasu/utils") &&
        source.imports.some((specifier) => specifier.startsWith("@tauri-apps/"))
      ) {
        issues.add(`${pkg.name}: Tauri import in ${source.path}`);
      }
    }
  }

  const visited = new Set<string>();
  for (const pkg of inventory) {
    visitWorkspaceDependencies(pkg.name, packages, [], visited, issues);
  }

  return [...issues].sort();
}
