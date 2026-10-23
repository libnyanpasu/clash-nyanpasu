import { stat } from "node:fs/promises";
import { isNotFound } from "./errors.ts";

/** Whether a path exists; errors other than a missing path propagate. */
export async function pathExists(target: string): Promise<boolean> {
  try {
    await stat(target);
    return true;
  } catch (error) {
    if (isNotFound(error)) return false;
    throw error;
  }
}
