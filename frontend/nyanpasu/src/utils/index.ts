import { isIpcError, rpcDomainError, RpcFailure } from '@nyanpasu/rpc'
import { ipcErrorMessage } from './ipc-error'
import type { ProfileLabel } from './profile-label'

export async function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

/** The simplest message for a caught error; the original goes to "copy error details". */
export function formatError(err: unknown, profileLabel?: ProfileLabel): string {
  const domain = err instanceof RpcFailure ? rpcDomainError(err) : err

  if (isIpcError(domain)) {
    return ipcErrorMessage(domain, profileLabel)
  }
  return err instanceof Error ? err.message : String(err)
}
