import '../../theme/src/styles/theme.css'
import '@/assets/styles/tailwind.css'
import { expect, onTestFinished, test } from 'vitest'
import { render } from 'vitest-browser-react'
import { CircularProgress, LinearProgress } from '@nyanpasu/ui/progress'

test('LinearProgress renders determinate M3E track, gap, and stop indicator', async () => {
  const view = await render(<LinearProgress value={45} aria-label="Download" />)
  onTestFinished(() => view.unmount())

  const root = view.getByRole('progressbar', { name: 'Download' }).element()
  expect(root).toHaveAttribute('aria-valuenow', '45')
  expect(root).toHaveAttribute('data-variant', 'flat')

  const indicator = root.querySelector(
    '[data-slot="linear-progress-indicator"]',
  )
  const track = root.querySelector('[data-slot="linear-progress-track"]')
  const stop = root.querySelector(
    '[data-slot="linear-progress-stop-indicator"]',
  )

  expect(indicator).not.toBeNull()
  expect(track).not.toBeNull()
  expect(stop).not.toBeNull()
})

test('LinearProgress hides track and stop indicator at 100% and supports stopIndicator={false}', async () => {
  const view = await render(
    <>
      <LinearProgress value={100} aria-label="Complete" />
      <LinearProgress
        value={50}
        stopIndicator={false}
        aria-label="No stop dot"
      />
    </>,
  )
  onTestFinished(() => view.unmount())

  const complete = view.getByRole('progressbar', { name: 'Complete' }).element()
  expect(
    complete.querySelector('[data-slot="linear-progress-track"]'),
  ).toBeNull()
  expect(
    complete.querySelector('[data-slot="linear-progress-stop-indicator"]'),
  ).toBeNull()

  const noStop = view
    .getByRole('progressbar', { name: 'No stop dot' })
    .element()
  expect(
    noStop.querySelector('[data-slot="linear-progress-track"]'),
  ).not.toBeNull()
  expect(
    noStop.querySelector('[data-slot="linear-progress-stop-indicator"]'),
  ).toBeNull()
})

test('LinearProgress renders wavy determinate and indeterminate variants', async () => {
  const view = await render(
    <>
      <LinearProgress
        variant="wavy"
        size="lg"
        value={60}
        aria-label="Wavy determinate"
      />
      <LinearProgress
        variant="wavy"
        indeterminate
        aria-label="Wavy indeterminate"
      />
    </>,
  )
  onTestFinished(() => view.unmount())

  const wavyDet = view
    .getByRole('progressbar', { name: 'Wavy determinate' })
    .element()
  expect(wavyDet).toHaveAttribute('data-variant', 'wavy')
  expect(wavyDet.querySelector('svg pattern path')).not.toBeNull()

  const wavyIndet = view
    .getByRole('progressbar', { name: 'Wavy indeterminate' })
    .element()
  expect(wavyIndet).toHaveAttribute('data-variant', 'wavy')
  expect(wavyIndet).toHaveAttribute('data-indeterminate', 'true')
  expect(wavyIndet).not.toHaveAttribute('aria-valuenow')
  expect(
    wavyIndet.querySelector(
      '[data-slot="linear-progress-indeterminate-primary"] svg',
    ),
  ).not.toBeNull()
})

test('CircularProgress renders determinate flat and wavy variants with M3E track', async () => {
  const view = await render(
    <>
      <CircularProgress value={65} aria-label="Circular flat">
        <span>65%</span>
      </CircularProgress>
      <CircularProgress
        variant="wavy"
        size="md"
        thickness="thick"
        value={50}
        aria-label="Circular wavy"
      />
    </>,
  )
  onTestFinished(() => view.unmount())

  const flat = view
    .getByRole('progressbar', { name: 'Circular flat' })
    .element()
  expect(flat).toHaveAttribute('aria-valuenow', '65')
  expect(flat).toHaveAttribute('data-variant', 'flat')
  expect(
    flat.querySelector('[data-slot="circular-progress-track"]'),
  ).not.toBeNull()
  expect(
    flat.querySelector('[data-slot="circular-progress-determinate-circle"]'),
  ).not.toBeNull()
  expect(flat.textContent).toContain('65%')

  const wavy = view
    .getByRole('progressbar', { name: 'Circular wavy' })
    .element()
  expect(wavy).toHaveAttribute('data-variant', 'wavy')
  expect(
    wavy.querySelector(
      'path[data-slot="circular-progress-determinate-circle"]',
    ),
  ).not.toBeNull()
})

test('CircularProgress renders indeterminate flat and wavy variants', async () => {
  const view = await render(
    <>
      <CircularProgress indeterminate aria-label="Spinner flat" />
      <CircularProgress
        variant="wavy"
        indeterminate
        showTrack={false}
        aria-label="Spinner wavy"
      />
    </>,
  )
  onTestFinished(() => view.unmount())

  const flat = view.getByRole('progressbar', { name: 'Spinner flat' }).element()
  expect(flat).toHaveAttribute('data-indeterminate', 'true')
  expect(flat).not.toHaveAttribute('aria-valuenow')
  expect(
    flat.querySelector('[data-slot="circular-progress-track"]'),
  ).not.toBeNull()

  const wavy = view.getByRole('progressbar', { name: 'Spinner wavy' }).element()
  expect(wavy.querySelector('[data-slot="circular-progress-track"]')).toBeNull()
  expect(
    wavy.querySelector(
      'path[data-slot="circular-progress-indeterminate-circle"]',
    ),
  ).not.toBeNull()
})
