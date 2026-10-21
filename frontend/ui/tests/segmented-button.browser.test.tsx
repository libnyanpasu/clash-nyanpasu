import '../../theme/src/styles/theme.css'
import '@/assets/styles/tailwind.css'
import { Profiler } from 'react'
import { expect, onTestFinished, test } from 'vitest'
import { render } from 'vitest-browser-react'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'

const frame = () =>
  new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)))

test('the tabs do not render their items for each step of placing the pill', async () => {
  let itemRenders = 0
  const view = await render(
    <SegmentedButton variant="tabs" value="b" aria-label="Tabs">
      {['a', 'b', 'c'].map((value) => (
        <Profiler key={value} id={value} onRender={() => (itemRenders += 1)}>
          <SegmentedButtonItem value={value}>{value}</SegmentedButtonItem>
        </Profiler>
      ))}
    </SegmentedButton>,
  )
  onTestFinished(() => view.unmount())

  await expect
    .poll(() =>
      document.querySelector('[data-slot="segmented-button-slider-pill"]'),
    )
    .toBeTruthy()
  // The resize observation and the pill's first animation come after.
  for (let i = 0; i < 5; i++) await frame()

  // Each item mounts, and renders once more with the toggle group when the
  // measured pill renders it again.
  expect(itemRenders).toBe(6)
})
