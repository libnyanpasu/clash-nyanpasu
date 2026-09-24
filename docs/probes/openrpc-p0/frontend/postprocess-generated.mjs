import { readFile, writeFile } from "node:fs/promises";

const file = new URL("./generated/client/typescript/src/index.ts", import.meta.url);
const source = await readFile(file, "utf8");
const output = source
  .replace(
  /^  public ([A-Za-z0-9_.]+): ([A-Za-z0-9_]+) =/gm,
  (_, methodName, methodType) => `  public [${JSON.stringify(methodName)}]: ${methodType} =`,
  )
  .replace(
    'import {\n  OpenrpcDocument as OpenRPC,\n  MethodObject,\n} from "@open-rpc/meta-schema";',
    'import type {\n  OpenrpcDocument as OpenRPC,\n  MethodObject,\n} from "@open-rpc/meta-schema";',
  );

if (output === source || output.includes("public nyanpasu.")) {
  throw new Error("OpenRPC generator output did not match the dotted method workaround");
}

await writeFile(file, output);
