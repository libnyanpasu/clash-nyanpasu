import { afterEach, describe, expect, it, vi } from 'vitest'
import { getClientId, withClientId } from '../src/client-id'

afterEach(() => vi.unstubAllGlobals())

function stubSessionStorage() {
  const values = new Map<string, string>()
  vi.stubGlobal('sessionStorage', {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
  })
  return values
}

describe('client id', () => {
  it('is generated once per tab and persisted in sessionStorage', () => {
    const values = stubSessionStorage()

    const id = getClientId()

    expect(id).toMatch(/^[0-9a-f-]{36}$/)
    expect(values.get('nyanpasu-client-id')).toBe(id)
    expect(getClientId()).toBe(id)
  })

  it('reuses an id a reload left behind', () => {
    stubSessionStorage().set('nyanpasu-client-id', 'existing-tab')

    expect(getClientId()).toBe('existing-tab')
  })

  it('stays stable in memory when sessionStorage throws', () => {
    vi.stubGlobal('sessionStorage', {
      getItem: () => {
        throw new DOMException('denied', 'SecurityError')
      },
      setItem: () => {
        throw new DOMException('denied', 'SecurityError')
      },
    })

    const id = getClientId()

    expect(id).toMatch(/^[0-9a-f-]{36}$/)
    expect(getClientId()).toBe(id)
  })

  it('appends the id to stream URLs with or without a query', () => {
    stubSessionStorage().set('nyanpasu-client-id', 'tab 1')

    expect(withClientId('/bridge/events')).toBe(
      '/bridge/events?client_id=tab%201',
    )
    expect(withClientId('/bridge/events?name=x')).toBe(
      '/bridge/events?name=x&client_id=tab%201',
    )
  })
})
