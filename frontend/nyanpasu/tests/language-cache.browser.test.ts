import { afterEach, expect, test, vi } from 'vitest'
import { m } from '@/paraglide/messages'
import { getLocale, setLocale } from '@/paraglide/runtime'
import '../src/utils/language'

afterEach(() => vi.restoreAllMocks())

test('messages resolve the locale without reading the storage', () => {
  const previous = getLocale()
  setLocale('zh-cn', { reload: false })

  try {
    const read = vi.spyOn(Storage.prototype, 'getItem')

    for (let i = 0; i < 100; i++) m.common_close()

    expect(read).not.toHaveBeenCalled()
    expect(getLocale()).toBe('zh-cn')
    expect(document.documentElement.lang).toBe('zh-cn')
  } finally {
    setLocale(previous, { reload: false })
  }

  expect(getLocale()).toBe(previous)
  expect(document.documentElement.lang).toBe(previous)
})
