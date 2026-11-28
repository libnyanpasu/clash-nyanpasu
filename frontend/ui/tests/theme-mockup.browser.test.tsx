import { expect, test } from 'vitest'
import { render } from 'vitest-browser-react'
import { ThemeMockup } from '@nyanpasu/ui/theme-mockup'
import { createTheme, ThemeMode } from '@nyanpasu/theme'

test('paints the mockup with its own scheme, not the page theme', async ({
  onTestFinished,
}) => {
  const { palette } = createTheme('#3e64a7')

  const view = await render(
    <div className="dark">
      <ThemeMockup
        palette={palette}
        mode={ThemeMode.LIGHT}
        aria-label="Light preview"
      />
      <ThemeMockup
        palette={palette}
        mode={ThemeMode.DARK}
        aria-label="Dark preview"
      />
    </div>,
  )
  onTestFinished(() => view.unmount())

  const light = view.getByRole('img', { name: 'Light preview' }).element()
  const dark = view.getByRole('img', { name: 'Dark preview' }).element()

  const primaryContainer = (element: Element) =>
    getComputedStyle(element).getPropertyValue('--color-primary-container')

  // Each mockup scopes the scheme of its own mode to itself.
  expect(primaryContainer(light)).not.toBe('')
  expect(primaryContainer(light)).not.toBe(primaryContainer(dark))
  expect(light.getAttribute('data-theme-mockup')).not.toBe(
    dark.getAttribute('data-theme-mockup'),
  )
})
