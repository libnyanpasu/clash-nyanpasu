import type { DownloadEvent } from '@tauri-apps/plugin-updater'

export type UpdateDownloadCandidate<T> = {
  source: string
  update: T
}

export async function downloadUpdateWithFallback<
  T extends {
    download: (onEvent?: (event: DownloadEvent) => void) => Promise<void>
  },
>(
  candidates: readonly UpdateDownloadCandidate<T>[],
  onEvent?: (event: DownloadEvent) => void,
): Promise<T> {
  const errors: string[] = []

  for (const { source, update } of candidates) {
    try {
      await update.download(onEvent)
      return update
    } catch (error) {
      errors.push(`${source}: ${String(error)}`)
    }
  }

  throw new Error(`All update package sources failed:\n${errors.join('\n')}`)
}
