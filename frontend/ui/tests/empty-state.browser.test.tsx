import '../../theme/src/styles/theme.css'
import '@/assets/styles/tailwind.css'
import { type ComponentProps } from 'react'
import { expect, onTestFinished, test } from 'vitest'
import { render } from 'vitest-browser-react'
import { EmptyState } from '@nyanpasu/ui/empty-state'

function TestIcon(props: ComponentProps<'svg'>) {
  return (
    <svg {...props} viewBox="0 0 24 24">
      <path fill="currentColor" d="M4 4h16v16H4z" />
    </svg>
  )
}

function luminance(color: string) {
  const channels = color
    .match(/[\d.]+/g)!
    .slice(0, 3)
    .map(Number)
  const linear = channels.map((channel) => {
    const value = channel / 255
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4
  })

  return linear[0]! * 0.2126 + linear[1]! * 0.7152 + linear[2]! * 0.0722
}

for (const dark of [false, true]) {
  test(`normal empty state stays readable on ${dark ? 'dark' : 'light'} panels`, async () => {
    const wasDark = document.documentElement.classList.contains('dark')
    document.documentElement.classList.toggle('dark', dark)
    onTestFinished(() => {
      document.documentElement.classList.toggle('dark', wasDark)
    })

    const view = await render(
      <div className="bg-surface w-64 rounded-xl p-3" data-testid="panel">
        <EmptyState
          icon={TestIcon}
          size="compact"
          tone="normal"
          message="No latency history yet"
        />
      </div>,
    )
    onTestFinished(() => view.unmount())

    const panel = view.getByTestId('panel').element()
    const message = panel.querySelector('[data-slot="empty-state-message"]')!
    const icon = panel.querySelector('[data-slot="empty-state-icon"]')!
    const foreground = getComputedStyle(message).color
    const background = getComputedStyle(panel).backgroundColor
    const levels = [luminance(foreground), luminance(background)].sort(
      (a, b) => a - b,
    )

    expect((levels[1]! + 0.05) / (levels[0]! + 0.05)).toBeGreaterThanOrEqual(
      4.5,
    )
    expect(getComputedStyle(icon).color).toBe(foreground)
    expect(panel.scrollWidth).toBeLessThanOrEqual(panel.clientWidth)
  })
}
