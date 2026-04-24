import type { InspectUpdater } from '../service/types'
import { unwrapResult } from '../utils'
import { commands } from './bindings'

export * from './consts'
export * from './use-server-port'
export * from './use-clash-config'
export * from './use-clash-connections'
export * from './use-clash-cores'
export * from './use-clash-info'
export * from './use-clash-logs'
export * from './use-clash-memory'
export * from './use-clash-proxies-provider'
export * from './use-clash-proxies'
export * from './use-clash-rules-provider'
export * from './use-clash-rules'
export * from './use-clash-traffic'
export * from './use-clash-version'
export * from './use-post-processing-output'
export * from './use-profile-content'
export * from './use-profile'
export * from './use-proxy-mode'
export * from './use-runtime-profile'
export * from './use-settings'
export * from './use-system-proxy'
export * from './use-system-service'
export * from './use-service-prompt'
export * from './use-core-dir'
export * from './use-platform'

export { commands, events } from './bindings'
export type * from './bindings'

const mapInspectUpdater = (
  result: Awaited<ReturnType<typeof commands.inspectUpdater>> extends infer R
    ? R extends { status: 'ok'; data: infer T }
      ? T
      : never
    : never,
): InspectUpdater => ({
  id: result.id,
  state:
    typeof result.state === 'string'
      ? result.state
      : { failed: result.state.failed },
  downloader: {
    ...result.downloader,
    chunks: result.downloader.chunks.map((chunk) => ({
      ...chunk,
      state: chunk.state.toLowerCase() as 'idle' | 'downloading' | 'finished',
    })),
  },
})

// manually added
export const openUWPTool = commands.invokeUwpTool
export const inspectUpdater = async (
  updaterId: number,
): Promise<InspectUpdater> =>
  mapInspectUpdater(unwrapResult(await commands.inspectUpdater(updaterId))!)
export const openThat = async (path: string): Promise<void> => {
  await unwrapResult(await commands.openThat(path))
}
export const isPortable = async (): Promise<boolean> =>
  unwrapResult(await commands.isPortable()) ?? false
export const isAppImage = async (): Promise<boolean> =>
  unwrapResult(await commands.isAppimage()) ?? false
export const getStorageItem = async (key: string): Promise<string | null> =>
  unwrapResult(await commands.getStorageItem(key)) ?? null
export const setStorageItem = async (
  key: string,
  value: string,
): Promise<void> => {
  await unwrapResult(await commands.setStorageItem(key, value))
}
export const removeStorageItem = async (key: string): Promise<void> => {
  await unwrapResult(await commands.removeStorageItem(key))
}
