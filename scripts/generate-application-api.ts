import { dirname, fromFileUrl, join } from "jsr:@std/path";

type Procedure = {
  fn_name: string;
  input_type: string;
  output_type: string;
  kind: "unary" | "stream";
};

const root = dirname(dirname(fromFileUrl(import.meta.url)));
const catalogPath = join(root, "backend/tauri/gen/application-api.json");
const typesPath = join(root, "frontend/interface/src/application-api/types.ts");
const outputPath = join(
  root,
  "frontend/interface/src/application-api/generated.ts",
);
const check = Deno.args.includes("--check");
if (Deno.args.some((arg) => arg !== "--check")) {
  throw new Error("Expected only --check");
}

const catalog = JSON.parse(await Deno.readTextFile(catalogPath)) as {
  procedures: Procedure[];
};
if (!Array.isArray(catalog.procedures)) {
  throw new Error("Application API catalog has no procedures");
}

const names = new Set<string>();
for (const procedure of catalog.procedures) {
  if (
    !/^[a-z][a-zA-Z0-9]*(\.[a-z][a-zA-Z0-9]*)+$/.test(
      procedure.fn_name,
    ) ||
    !/^[A-Za-z_$][\w$]*$/.test(procedure.input_type) &&
      procedure.input_type !== "null" ||
    !/^[A-Za-z_$][\w$]*$/.test(procedure.output_type) ||
    !["unary", "stream"].includes(procedure.kind) ||
    names.has(procedure.fn_name)
  ) {
    throw new Error(`Invalid or duplicate procedure ${procedure.fn_name}`);
  }
  names.add(procedure.fn_name);
}

const procedures = [...catalog.procedures].sort((a, b) =>
  a.fn_name.localeCompare(b.fn_name)
);
const typeMap = (kind: Procedure["kind"]) => {
  const entries = procedures
    .filter((procedure) => procedure.kind === kind)
    .map(
      (procedure) =>
        `  ${
          JSON.stringify(procedure.fn_name)
        }: { params: ${procedure.input_type}; ${
          kind === "unary" ? "result" : "event"
        }: ${procedure.output_type} }`,
    );
  return entries.join("\n");
};
const types = await Deno.readTextFile(typesPath);
const importedTypes = [
  ...new Set(
    procedures.flatMap((procedure) =>
      [procedure.input_type, procedure.output_type].filter((name) =>
        name !== "null"
      )
    ),
  ),
].sort();
for (const name of importedTypes) {
  if (!types.includes(`export type ${name} =`)) {
    throw new Error(`Specta did not export ${name}`);
  }
}
const generated =
  `/* Generated from backend/tauri/gen/application-api.json. Do not edit. */
import type { ${importedTypes.join(", ")} } from "./types";
export type * from "./types";

export type ApplicationApiUnary = {
${typeMap("unary")}
}

export type ApplicationApiStreams = {
${typeMap("stream")}
}

export const applicationApiProcedureNames = ${
    JSON.stringify(
      procedures.map((procedure) => procedure.fn_name),
    )
  } as const
`;
const current = await Deno.readTextFile(outputPath).catch(() => "");
if (check) {
  if (current !== generated) {
    throw new Error(
      "Application API client is stale; run pnpm generate:application-api",
    );
  }
} else if (current !== generated) {
  await Deno.mkdir(dirname(outputPath), { recursive: true });
  await Deno.writeTextFile(outputPath, generated);
}
