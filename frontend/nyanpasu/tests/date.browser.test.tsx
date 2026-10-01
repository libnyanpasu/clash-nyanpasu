import { createRoot } from 'react-dom/client'
import { expect, onTestFinished, test, vi } from 'vitest'
import { getLocale, setLocale } from '@/paraglide/runtime'
import '../src/utils/language'
import { RelativeTimeCell } from '../src/pages/(main)/main/connections/_modules/cells'

test.each([
  ['en', '10 minutes ago'],
  ['ko', '10분 전'],
  ['ru', '10 минут назад'],
  ['zh-cn', '10 分钟前'],
  ['zh-tw', '10 分鐘前'],
] as const)(
  'renders the first timestamp in %s and keeps the locale after remount',
  async (language, expected) => {
    const previousLocale = getLocale()
    setLocale(language, { reload: false })
    vi.useFakeTimers({ toFake: ['Date'] })
    const now = new Date(2026, 0, 1, 12)
    vi.setSystemTime(now)

    const container = document.createElement('div')
    document.body.append(container)
    let root = createRoot(container)
    onTestFinished(() => {
      root.unmount()
      container.remove()
      vi.useRealTimers()
      setLocale(previousLocale, { reload: false })
    })

    const render = () =>
      root.render(<RelativeTimeCell ms={now.getTime() - 600_000} />)
    render()
    await expect.poll(() => container.textContent).toBe(expected)
    expect(container.querySelector('span')?.title).toBe('2026-01-01 11:50:00')

    root.unmount()
    root = createRoot(container)
    render()
    await expect.poll(() => container.textContent).toBe(expected)
  },
)
