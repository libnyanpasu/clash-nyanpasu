import { expect, test } from 'vitest'
import { searchableText } from '../src/utils/searchable-text.ts'

const connection = {
  id: 'Conn-1',
  download: 2048,
  chains: ['Node-A', 'Proxy'],
  metadata: { host: 'Example.COM', processPath: '/usr/bin/curl', port: 443 },
  _extra: { sniffHost: 'cdn.example.net' },
}

test('holds every nested string, lowercased', () => {
  const text = searchableText(connection)

  for (const term of ['conn-1', 'node-a', 'example.com', 'bin/curl', 'cdn.']) {
    expect(text.includes(term)).toBe(true)
  }
})

test('holds no numbers, as the per-field search did not', () => {
  expect(searchableText(connection).includes('2048')).toBe(false)
})

test('a term never matches across two strings', () => {
  expect(searchableText(connection).includes('node-aproxy')).toBe(false)
  expect(searchableText({ a: 'ab', b: 'cd' }).includes('bc')).toBe(false)
})
