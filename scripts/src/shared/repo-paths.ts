import { fromFileUrl } from "jsr:@std/path";

/** Repository paths independent of the task caller and source file depth. */
export const WORKSPACE_ROOT = fromFileUrl(
  new URL("../../../", import.meta.url),
);
