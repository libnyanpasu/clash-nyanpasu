import assert from 'node:assert/strict'
import { test } from 'node:test'
import { mapArch, normalizeArch, normalizePlatform } from './platform.ts'

test('normalizes platform and architecture names', () => {
  assert.deepStrictEqual(normalizePlatform('windows'), 'win32')
  assert.deepStrictEqual(normalizePlatform('darwin'), 'darwin')
  assert.deepStrictEqual(normalizeArch('x86_64'), 'x64')
  assert.deepStrictEqual(normalizeArch('aarch64'), 'arm64')
  assert.deepStrictEqual(normalizeArch('ia32'), 'ia32')
})

test('maps supported platform and architecture pairs', () => {
  assert.deepStrictEqual(mapArch('darwin', 'arm64'), 'darwin-arm64')
  assert.deepStrictEqual(mapArch('win32', 'x64'), 'windows-x86_64')
  assert.deepStrictEqual(mapArch('linux', 'arm64'), 'linux-aarch64')
})

test('rejects unsupported platform and architecture pairs', () => {
  assert.throws(() => mapArch('freebsd', 'x64'), {
    message: /Unsupported platform\/architecture: freebsd-x64/,
  })
})
