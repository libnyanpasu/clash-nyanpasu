import { expect, test } from 'vitest'
import { isUpdaterSupported } from '../src/utils/updater-support.ts'

test.for([
  {
    name: 'windows installer',
    linux: false,
    appImage: false,
    portable: false,
    expected: true,
  },
  {
    name: 'windows portable',
    linux: false,
    appImage: false,
    portable: true,
    expected: false,
  },
  {
    name: 'macOS app',
    linux: false,
    appImage: false,
    portable: false,
    expected: true,
  },
  {
    name: 'linux AppImage',
    linux: true,
    appImage: true,
    portable: false,
    expected: true,
  },
  {
    name: 'linux deb/rpm',
    linux: true,
    appImage: false,
    portable: false,
    expected: false,
  },
  {
    name: 'platform still loading',
    linux: true,
    appImage: undefined,
    portable: undefined,
    expected: false,
  },
])('updater support for $name', ({ expected, ...platform }) => {
  expect(isUpdaterSupported({ tauri: true, ...platform })).toBe(expected)
})

test('the browser transport never offers in-place updates', () => {
  expect(
    isUpdaterSupported({
      tauri: false,
      linux: false,
      appImage: false,
      portable: false,
    }),
  ).toBe(false)
})
