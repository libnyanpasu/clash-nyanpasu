import { beforeEach, expect, test } from 'vitest'
import type { RpcDescribeSnapshot } from '@/components/auth/capabilities'
import { principalKey } from '@/components/auth/principal-marker'

type Identity = Pick<RpcDescribeSnapshot, 'principal' | 'scopes'>

beforeEach(() => localStorage.clear())

test('scope order does not change the principal key', () => {
  const principal = { kind: 'token', id: 'a1' }

  expect(principalKey({ principal, scopes: ['proxy', 'read'] })).toBe(
    principalKey({ principal, scopes: ['read', 'proxy'] }),
  )
})

test('principal kind, id and scopes each distinguish the key', () => {
  const base: Identity = {
    principal: { kind: 'token', id: 'a1' },
    scopes: ['read'],
  }
  const keys = new Set([
    principalKey(base),
    principalKey({ ...base, principal: { kind: 'session', id: 'a1' } }),
    principalKey({ ...base, principal: { kind: 'token', id: 'a2' } }),
    principalKey({ ...base, scopes: ['read', 'proxy'] }),
  ])

  expect(keys.size).toBe(4)
})

test('every desktop window is the same caller', () => {
  const scopes: Identity['scopes'] = ['read']

  expect(
    principalKey({ principal: { kind: 'desktop', id: 'main' }, scopes }),
  ).toBe(principalKey({ principal: { kind: 'desktop', id: 'tray' }, scopes }))
})

test('the key does not hold the display name', () => {
  expect(
    principalKey({
      principal: { kind: 'token', id: 'a1', name: 'secret-label' },
      scopes: [],
    }),
  ).not.toContain('secret-label')
})
