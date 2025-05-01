import type { ClashCore } from '../ipc/bindings'
import { fetchLatestCoreVersions, getCoreVersion } from './tauri'

export interface Core {
  name: string
  core: ClashCore
  version?: string
  latest?: string
}

export const VALID_CORE: Core[] = [
  { name: 'Clash Premium', core: 'clash' },
  { name: 'Mihomo', core: 'mihomo' },
  { name: 'Mihomo Alpha', core: 'mihomo-alpha' },
  { name: 'Clash Rust', core: 'clash-rs' },
  { name: 'Clash Rust Alpha', core: 'clash-rs-alpha' },
]

export const fetchCoreVersion = async () => {
  return await Promise.all(
    VALID_CORE.map(async (item) => {
      try {
        const version = await getCoreVersion(item.core)
        return { ...item, version }
      } catch (e) {
        console.error('failed to fetch core version', e)
        return { ...item, version: 'N/A' }
      }
    }),
  )
}

export const fetchLatestCore = async () => {
  const results = await fetchLatestCoreVersions()

  const cores = VALID_CORE.map((item) => {
    const key = item.core.replace(/-/g, '_') as keyof typeof results

    let latest: string

    switch (item.core) {
      case 'clash':
        latest = `n${results['clash_premium']}`
        break

      case 'clash-rs':
        latest = results[key].replace(/v/, '')
        break

      default:
        latest = results[key]
        break
    }

    return {
      ...item,
      latest,
    }
  })

  return cores
}

export enum SupportedArch {
  // blocked by clash-rs
  // WindowsX86 = "windows-x86",
  WindowsX86_64 = 'windows-x86_64',
  // blocked by clash-rs#212
  // WindowsArm64 = "windows-arm64",
  LinuxAarch64 = 'linux-aarch64',
  LinuxAmd64 = 'linux-amd64',
  DarwinArm64 = 'darwin-arm64',
  DarwinX64 = 'darwin-x64',
}

export enum SupportedCore {
  Mihomo = 'mihomo',
  MihomoAlpha = 'mihomo_alpha',
  ClashRs = 'clash_rs',
  ClashPremium = 'clash_premium',
}

export type ArchMapping = { [key in SupportedArch]: string }

export interface ManifestVersion {
  manifest_version: number
  latest: { [K in SupportedCore]: string }
  arch_template: { [K in SupportedCore]: ArchMapping }
  updated_at: string // ISO 8601
}
