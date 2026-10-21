import { m } from '@/paraglide/messages'
import type { CreditPackage, CreditSource } from './use-open-source-credits'

export const CREDIT_SOURCES: CreditSource[] = ['bundled', 'cargo', 'npm']

export const creditSourceLabel: Record<CreditSource, () => string> = {
  bundled: m.settings_about_credits_source_bundled,
  cargo: m.settings_about_credits_source_cargo,
  npm: m.settings_about_credits_source_npm,
}

const PLATFORM_NAMES = {
  windows: 'Windows',
  linux: 'Linux',
  macos: 'macOS',
} as const

export function creditPlatforms(platforms: CreditPackage['platforms']) {
  return platforms?.map((platform) => PLATFORM_NAMES[platform]).join(', ')
}
