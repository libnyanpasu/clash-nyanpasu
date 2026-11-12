import { expect, test, vi } from 'vitest'
import { convertFileSrc } from '@tauri-apps/api/core'
import { createIconTransport } from '../src/icon-transport'

vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc: vi.fn() }))

test('icon_url_parser_matches_frontend_fixtures', () => {
  const icons = createIconTransport(false)
  expect(
    icons.url({
      kind: 'cached',
      url: 'https://icons.example/猫 icon.svg?a=1&token=a+b&next=%2F',
    }),
  ).toBe(
    '/bridge/icons/cached?url=https%3A%2F%2Ficons.example%2F%E7%8C%AB+icon.svg%3Fa%3D1%26token%3Da%2Bb%26next%3D%252F',
  )
  expect(icons.url({ kind: 'tray', mode: 'normal', version: 0 })).toBe(
    '/bridge/icons/tray?mode=normal&version=0',
  )
  expect(
    icons.url({ kind: 'widget', mode: 'system_proxy', version: 4294967295 }),
  ).toBe('/bridge/icons/widget?mode=system_proxy&version=4294967295')
  expect(icons.url({ kind: 'tray', mode: 'tun', version: null })).toBe(
    '/bridge/icons/tray?mode=tun',
  )
})

test('desktop icons append query after converting only the fixed path', () => {
  const convert = vi.mocked(convertFileSrc)
  for (const base of [
    'http://nyanpasu-icons.localhost',
    'https://nyanpasu-icons.localhost',
    'nyanpasu-icons://localhost',
  ]) {
    convert.mockImplementation((path) => `${base}/${path}`)
    expect(
      createIconTransport(true).url({
        kind: 'cached',
        url: 'https://icons.example/a?b=c&d=+',
      }),
    ).toBe(
      `${base}/cached?url=https%3A%2F%2Ficons.example%2Fa%3Fb%3Dc%26d%3D%2B`,
    )
    expect(convert).toHaveBeenLastCalledWith('cached', 'nyanpasu-icons')
  }
})
