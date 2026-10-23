import { fileURLToPath } from "node:url";

/** Repository paths independent of the task caller and source file depth. */
export const WORKSPACE_ROOT = fileURLToPath(
  new URL("../../../", import.meta.url),
);
