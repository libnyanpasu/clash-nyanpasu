import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import * as path from 'node:path'
import { test } from 'node:test'
import { WORKSPACE_ROOT } from '../shared/repo-paths.ts'
import {
  createVersionManifest,
  resolveMeowAlphaRelease,
  type ArchMapping,
  type GitHubRelease,
  type ResolvedCoreVersion,
  type SupportedCore,
} from './manifest.ts'

const activeCoreNames = [
  'mihomo',
  'mihomo_alpha',
  'clash_rs',
  'clash_rs_alpha',
  'meow',
  'meow_alpha',
] as const satisfies readonly SupportedCore[]

const legacyClashPremiumArchMapping: ArchMapping = {
  'windows-x86_64': 'clash-windows-amd64-n{}.zip',
  'windows-arm64': 'clash-windows-arm64-n{}.zip',
  'linux-aarch64': 'clash-linux-arm64-n{}.gz',
  'linux-amd64': 'clash-linux-amd64-n{}.gz',
  'darwin-arm64': 'clash-darwin-arm64-n{}.gz',
  'darwin-x64': 'clash-darwin-amd64-n{}.gz',
}

function archMapping(name: string): ArchMapping {
  return {
    'windows-x86_64': `${name}-windows-x86_64`,
    'windows-arm64': `${name}-windows-arm64`,
    'linux-aarch64': `${name}-linux-aarch64`,
    'linux-amd64': `${name}-linux-amd64`,
    'darwin-arm64': `${name}-darwin-arm64`,
    'darwin-x64': `${name}-darwin-x64`,
  }
}

function activeVersions(): ResolvedCoreVersion[] {
  return activeCoreNames.map((name) => ({
    name,
    version: `test-${name}`,
    archMapping: archMapping(name),
  }))
}

const alphaRelease: GitHubRelease = {
  tag_name: 'Prerelease-Alpha',
  prerelease: true,
  assets: [
    { name: 'meow-alpha-3c27aca-aarch64-apple-darwin.tar.gz' },
    { name: 'meow-alpha-3c27aca-aarch64-pc-windows-msvc.zip' },
    { name: 'meow-alpha-3c27aca-aarch64-unknown-linux-musl.tar.gz' },
    { name: 'meow-alpha-3c27aca-x86_64-apple-darwin.tar.gz' },
    { name: 'meow-alpha-3c27aca-x86_64-pc-windows-msvc.zip' },
    { name: 'meow-alpha-3c27aca-x86_64-unknown-linux-musl.tar.gz' },
  ],
}

test('resolve Meow Alpha from all official release assets', () => {
  const result = resolveMeowAlphaRelease(alphaRelease)
  assert.deepStrictEqual(result.name, 'meow_alpha')
  assert.deepStrictEqual(result.version, 'alpha-3c27aca')
  assert.deepStrictEqual(
    result.archMapping['darwin-arm64'],
    'meow-{}-aarch64-apple-darwin.tar.gz',
  )
  assert.deepStrictEqual(
    result.archMapping['linux-amd64'],
    'meow-{}-x86_64-unknown-linux-musl.tar.gz',
  )
})

test('reject incomplete or mixed-commit Meow Alpha release assets', () => {
  assert.throws(() =>
    resolveMeowAlphaRelease({
      ...alphaRelease,
      assets: alphaRelease.assets.slice(1),
    }),
  )

  const mixedRelease = structuredClone(alphaRelease)
  mixedRelease.assets[0].name = mixedRelease.assets[0].name.replace(
    '3c27aca',
    'deadbee',
  )
  assert.throws(() => resolveMeowAlphaRelease(mixedRelease))
})

test('generated manifest keeps the fields required by older clients', () => {
  const manifest = createVersionManifest(activeVersions())

  assert.strictEqual(manifest.latest.clash_premium, '2023-09-05-gdcc8d87')
  assert.deepStrictEqual(
    manifest.arch_template.clash_premium,
    legacyClashPremiumArchMapping,
  )
})

test('checked-in manifest keeps the fields required by older clients', async () => {
  const manifest = JSON.parse(
    await readFile(path.join(WORKSPACE_ROOT, 'manifest/version.json'), 'utf8'),
  )

  assert.strictEqual(manifest.latest.clash_premium, '2023-09-05-gdcc8d87')
  assert.deepStrictEqual(
    manifest.arch_template.clash_premium,
    legacyClashPremiumArchMapping,
  )
})
