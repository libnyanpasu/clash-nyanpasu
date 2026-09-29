import type { IpcError } from '@nyanpasu/interface'

/** The simplest message for a failed command, localized by its domain. */
export function ipcErrorMessage(error: IpcError): string {
  switch (error.kind.domain) {
    case 'unknown':
      return error.message
  }
}
