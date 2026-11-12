import { convertFileSrc } from '@tauri-apps/api/core'
import type { IconRequest } from './types'

/** Builds image URLs without invoking application commands. */
export interface RpcIconTransport {
  url(request: IconRequest): string
}

/** Query encoding follows the shared backend icon parser on both transports. */
export function createIconTransport(desktop: boolean): RpcIconTransport {
  return {
    url(request) {
      const query = new URLSearchParams()
      if (request.kind === 'cached') {
        query.set('url', request.url)
      } else {
        query.set('mode', request.mode)
        if (request.version != null) {
          query.set('version', String(request.version))
        }
      }

      // convertFileSrc encodes its input as a path; append the query afterwards.
      const base = desktop
        ? convertFileSrc(request.kind, 'nyanpasu-icons')
        : `/bridge/icons/${request.kind}`

      return `${base}?${query.toString()}`
    },
  }
}
