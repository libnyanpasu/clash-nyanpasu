import { expect, test } from 'vitest'
import {
  canUpdateCoreVersion,
  hasNewerCoreVersion,
} from '../src/ipc/core-version'

test('stable SemVer ignores an optional v and only accepts a higher remote version', () => {
  expect(hasNewerCoreVersion('mihomo', 'v0.22.0', '0.22.0')).toBe(false)
  expect(hasNewerCoreVersion('mihomo', 'v0.22.0', 'v0.22.1')).toBe(true)
  expect(hasNewerCoreVersion('mihomo', '0.22.1', 'v0.22.0')).toBe(false)
  expect(hasNewerCoreVersion('mihomo', 'v0.22.0', 'N/A')).toBe(false)
})

test('rolling cores compare build hashes even when the SemVer base is unchanged', () => {
  expect(
    hasNewerCoreVersion(
      'clash-rs-alpha',
      'v0.22.0-alpha+sha.abcdef0',
      '0.22.0-alpha+sha.abcdef0',
    ),
  ).toBe(false)
  expect(
    hasNewerCoreVersion(
      'clash-rs-alpha',
      'v0.22.0-alpha+sha.abcdef0',
      '0.22.0-alpha+sha.1234567',
    ),
  ).toBe(true)
  expect(
    hasNewerCoreVersion('meow-alpha', '0.22.0-alpha+3c27aca', 'alpha-3c27aca'),
  ).toBe(false)
  expect(
    hasNewerCoreVersion('meow-alpha', '0.22.0-alpha+3c27aca', 'alpha-5ed85c1'),
  ).toBe(true)
  expect(
    hasNewerCoreVersion('mihomo-alpha', 'alpha-abcdef0', 'alpha-abcdef0'),
  ).toBe(false)
  expect(
    hasNewerCoreVersion('mihomo-alpha', 'alpha-abcdef0', 'alpha-1234567'),
  ).toBe(true)
})

test('dated Clash Premium releases compare dates and same-day hashes', () => {
  expect(
    hasNewerCoreVersion('clash', '2023-09-05-gabcdef0', 'n2023-09-04-g1234567'),
  ).toBe(false)
  expect(
    hasNewerCoreVersion('clash', '2023-09-05-gabcdef0', '2023-09-05-gabcdef0'),
  ).toBe(false)
  expect(
    hasNewerCoreVersion('clash', '2023-09-05-gabcdef0', '2023-09-05-g1234567'),
  ).toBe(true)
})

test('unknown and unreadable versions do not report an update', () => {
  expect(
    hasNewerCoreVersion('meow-alpha', 'N/A', '0.22.0-alpha+sha.1234567'),
  ).toBe(false)
  expect(hasNewerCoreVersion('meow-alpha', 'not-a-version', '0.22.0')).toBe(
    false,
  )
  expect(hasNewerCoreVersion('mihomo', '0.22.0', 'development')).toBe(false)
})

test('a failed version read offers repair only when a valid remote version exists', () => {
  expect(canUpdateCoreVersion('meow-alpha', 'N/A', 'alpha-3c27aca', true)).toBe(
    true,
  )
  expect(canUpdateCoreVersion('meow-alpha', 'N/A', 'N/A', true)).toBe(false)
  expect(canUpdateCoreVersion('meow-alpha', 'N/A', 'not-a-version', true)).toBe(
    false,
  )
  expect(
    canUpdateCoreVersion('meow-alpha', 'N/A', 'alpha-3c27aca', false),
  ).toBe(false)
})
