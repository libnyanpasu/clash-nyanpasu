import { beforeEach, expect, test } from 'vitest'
import type { RpcDescribeSnapshot } from '@/components/auth/capabilities'
import { principalKey } from '@/components/auth/principal-marker'

type Identity = Pick<RpcDescribeSnapshot, 'principal' | 'permissions'>

beforeEach(() => localStorage.clear())

test('permission order does not change the principal key', () => {
  const principal: Identity['principal'] = {
    kind: 'token',
    id: 'a1',
    name: 'laptop',
  }

  expect(
    principalKey({
      principal,
      permissions: { proxy: 'write', logs: 'read' },
    }),
  ).toBe(
    principalKey({
      principal,
      permissions: { logs: 'read', proxy: 'write' },
    }),
  )
})

test('principal kind, id and permissions each distinguish the key', () => {
  const base: Identity = {
    principal: { kind: 'token', id: 'a1', name: 'laptop' },
    permissions: { proxy: 'read' },
  }
  const keys = new Set([
    principalKey(base),
    principalKey({ ...base, principal: { kind: 'session', id: 'a1' } }),
    principalKey({
      ...base,
      principal: { kind: 'token', id: 'a2', name: 'phone' },
    }),
    principalKey({
      ...base,
      principal: { kind: 'local_socket', user: '1000' },
    }),
    principalKey({ ...base, principal: { kind: 'anonymous' } }),
    principalKey({ ...base, permissions: { proxy: 'write' } }),
  ])

  expect(keys.size).toBe(6)
})

test('every desktop window is the same caller', () => {
  const permissions: Identity['permissions'] = { proxy: 'read' }

  expect(
    principalKey({
      principal: { kind: 'desktop', window: 'main' },
      permissions,
    }),
  ).toBe(
    principalKey({
      principal: { kind: 'desktop', window: 'tray' },
      permissions,
    }),
  )
})

test('the key does not hold the display name', () => {
  expect(
    principalKey({
      principal: { kind: 'token', id: 'a1', name: 'secret-label' },
      permissions: {},
    }),
  ).not.toContain('secret-label')
})
