import '../../theme/src/styles/theme.css'
import '@/assets/styles/tailwind.css'
import { expect, onTestFinished, test } from 'vitest'
import { render } from 'vitest-browser-react'
import { LoadingIndicator } from '@nyanpasu/ui/loading-indicator'

test('LoadingIndicator renders default and contained M3E variants', async () => {
  const view = await render(
    <>
      <LoadingIndicator aria-label="Loading default" />
      <LoadingIndicator
        variant="contained"
        size="lg"
        aria-label="Loading contained"
      />
    </>,
  )
  onTestFinished(() => view.unmount())

  const defaultIndicator = view
    .getByRole('progressbar', { name: 'Loading default' })
    .element()
  expect(defaultIndicator).toHaveAttribute('data-variant', 'default')
  expect(defaultIndicator).toHaveAttribute('data-size', 'md')
  expect(defaultIndicator).toHaveAttribute('aria-busy', 'true')
  expect(
    defaultIndicator.querySelector('[data-slot="loading-indicator-shape"]'),
  ).not.toBeNull()

  const containedIndicator = view
    .getByRole('progressbar', { name: 'Loading contained' })
    .element()
  expect(containedIndicator).toHaveAttribute('data-variant', 'contained')
  expect(containedIndicator).toHaveAttribute('data-size', 'lg')
  expect(
    containedIndicator.querySelector('[data-slot="loading-indicator-shape"]'),
  ).not.toBeNull()
})
