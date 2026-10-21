import { flushSync } from 'react-dom'
import { createRoot } from 'react-dom/client'
import { expect, test } from 'vitest'
import { Button } from '@nyanpasu/ui/button'
import '@/assets/styles/tailwind.css'

test.for(['transparent', 'rgb(100 100 100 / 20%)'])(
  'hides content while loading on a %s background without resizing',
  async (backgroundColor, { onTestFinished }) => {
    const container = document.createElement('div')
    document.body.append(container)
    const root = createRoot(container)
    onTestFinished(() => {
      root.unmount()
      container.remove()
    })

    for (const asChild of [false, true]) {
      const render = (loading: boolean) => {
        const label = (
          <>
            <svg width="20" height="20" aria-label="Refresh icon">
              <circle cx="10" cy="10" r="8" />
            </svg>
            Refresh
          </>
        )
        flushSync(() => {
          root.render(
            <Button
              asChild={asChild}
              loading={loading}
              disableRipple
              className="flex items-center gap-2"
              style={{ backgroundColor }}
            >
              {asChild ? <a href="#refresh">{label}</a> : label}
            </Button>,
          )
        })
      }

      render(false)
      const button = container.firstElementChild!
      const { width, height } = button.getBoundingClientRect()
      const icon = container.querySelector('svg')!
      expect(getComputedStyle(icon).visibility).toBe('visible')

      render(true)
      expect(getComputedStyle(icon).visibility).toBe('hidden')
      expect(
        getComputedStyle(
          container.querySelector('[data-slot="button-content"]')!,
        ).visibility,
      ).toBe('hidden')
      const progress = container.querySelector(
        '[data-slot="circular-progress"]',
      )!
      expect(getComputedStyle(progress).visibility).toBe('visible')
      expect(button.getBoundingClientRect().width).toBe(width)
      expect(button.getBoundingClientRect().height).toBe(height)

      await expect
        .poll(() =>
          Number(
            getComputedStyle(
              container.querySelector('[data-slot="button-loading"]')!,
            ).opacity,
          ),
        )
        .toBe(1)
      render(false)
      expect(
        container.querySelector('[data-slot="button-loading"]'),
      ).not.toBeNull()
      expect(getComputedStyle(icon).visibility).toBe('hidden')
      await expect
        .poll(() => container.querySelector('[data-slot="button-loading"]'))
        .toBeNull()
      expect(getComputedStyle(icon).visibility).toBe('visible')
    }
  },
)
