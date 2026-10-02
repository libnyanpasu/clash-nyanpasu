import { expect, test } from 'vitest'
import { digestKeys } from '../src/ipc/use-traffic-usage'

test('equal keys digest equally', () => {
  expect(digestKeys(['DomainSuffix,a.com', 'Match'])).toBe(
    digestKeys(['DomainSuffix,a.com', 'Match']),
  )
})

test('order, boundaries and count change the digest', () => {
  const digest = digestKeys(['ab', 'c'])

  expect(digestKeys(['c', 'ab'])).not.toBe(digest)
  expect(digestKeys(['a', 'bc'])).not.toBe(digest)
  expect(digestKeys(['ab', 'c', ''])).not.toBe(digest)
})

test('rule label sets of a large profile do not collide', () => {
  const digests = new Set(
    Array.from({ length: 2000 }, (_, i) =>
      digestKeys(
        Array.from({ length: 50 }, (_, j) => `DomainSuffix,site-${i + j}.com`),
      ),
    ),
  )

  expect(digests.size).toBe(2000)
})
