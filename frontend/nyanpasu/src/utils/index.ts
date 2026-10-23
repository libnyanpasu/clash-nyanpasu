import { isIpcError } from '@nyanpasu/rpc'
import { ipcErrorMessage } from './ipc-error'
import type { ProfileLabel } from './profile-label'

export async function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

/** The simplest message for a caught error; the original goes to "copy error details". */
export function formatError(err: unknown, profileLabel?: ProfileLabel): string {
  if (isIpcError(err)) {
    return ipcErrorMessage(err, profileLabel)
  }
  return err instanceof Error ? err.message : String(err)
}
