// Layout is measured with the app's Inter font, theme and Tailwind classes.
import '@fontsource-variable/inter'
import '@nyanpasu/theme/styles/fonts.css'
import '@nyanpasu/theme/styles/theme.css'
import '@/assets/styles/tailwind.css'
import { useState } from 'react'
import { expect, onTestFinished, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import FilterChips, {
  fitCount,
  type FilterChipItem,
} from '../src/pages/(main)/main/_modules/filter-chips'

vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({ isMinimized: async () => false }),
}))

// Icons take their real size here, so every chip has its real width.
vi.mock('./icon-stub', () => ({
  default: (props: object) => <svg {...props} />,
}))

test('every chip stays inline while they fit with the clear-all button', () => {
  expect(fitCount([100, 100], 80, 40, 8, 296)).toBe(2)
})

test('chips that do not fit leave room for the chip that opens the rest', () => {
  // 100 + 8 + 100 + 8 + 80 overflows 295. Both chips would fit beside the
  // opener, but then it would hide nothing and clear-all would be lost.
  expect(fitCount([100, 100], 80, 40, 8, 295)).toBe(1)
  expect(fitCount([100, 100], 80, 40, 8, 147)).toBe(0)
})

const LABELS = [
  'Host: www.bilibili.com',
  'Inbound: tun',
  'Process: curl.exe',
  'Rule: DomainSuffix,example.com',
]

function Harness({ onClearAll }: { onClearAll: () => void }) {
  const [labels, setLabels] = useState(LABELS)

  const items: FilterChipItem[] = labels.map((label) => ({
    key: label,
    label,
    onRemove: () => setLabels(labels.filter((item) => item !== label)),
  }))

  return (
    <div data-testid="frame" className="flex">
      <FilterChips
        className="flex-1"
        data-slot="test-filters"
        items={items}
        onClearAll={() => {
          onClearAll()
          setLabels([])
        }}
      />
    </div>
  )
}

async function open(width: number) {
  const onClearAll = vi.fn()
  const view = await render(<Harness onClearAll={onClearAll} />)
  onTestFinished(() => view.unmount())

  await document.fonts.ready

  const frame = view.getByTestId('frame').element() as HTMLElement
  frame.style.width = `${width}px`

  const group = frame.querySelector<HTMLElement>('[data-slot=test-filters]')!
  const inline = () =>
    [...group.children].filter(
      (child) => getComputedStyle(child).visibility !== 'hidden',
    )

  return { view, group, inline, onClearAll }
}

test('a wide row shows every chip and the clear-all button', async () => {
  const { view, inline } = await open(1200)

  await expect
    .element(view.getByRole('button', { name: m.traffic_filter_clear_all() }))
    .toBeVisible()
  expect(inline()).toHaveLength(LABELS.length + 1)
  expect(
    view
      .getByRole('button', {
        name: m.traffic_filter_show_all({ count: LABELS.length }),
      })
      .query(),
  ).toBeNull()
})

test('a narrow row collapses the rest into a dialog of every chip', async () => {
  const { view, group, inline, onClearAll } = await open(320)

  const more = view.getByRole('button', {
    name: m.traffic_filter_show_all({ count: LABELS.length }),
  })
  await expect.element(more).toBeVisible()

  // Whatever stays inline fits: nothing is cut at the edge. The opener can
  // show before the last resize is counted, so wait for the count to settle.
  const { right } = group.getBoundingClientRect()
  await expect
    .poll(() =>
      inline().every((child) => child.getBoundingClientRect().right <= right),
    )
    .toBe(true)

  const hidden = LABELS.length - (inline().length - 1)
  expect(hidden).toBeGreaterThan(0)
  expect(more.element().textContent).toBe(`+${hidden}`)

  await more.click()
  const dialog = view.getByRole('dialog')
  await expect.element(dialog).toBeVisible()
  for (const label of LABELS) {
    await expect.element(dialog.getByText(label)).toBeVisible()
  }

  // Removing in the dialog removes the filter; the dialog keeps the rest.
  await dialog
    .getByRole('button', {
      name: m.traffic_filter_remove({ filter: LABELS[3] }),
    })
    .click()
  await expect.element(dialog.getByText(LABELS[3])).not.toBeInTheDocument()
  await expect.element(dialog.getByText(LABELS[0])).toBeVisible()

  await dialog
    .getByRole('button', { name: m.traffic_filter_clear_all() })
    .click()
  expect(onClearAll).toHaveBeenCalledOnce()
  await expect.element(view.getByRole('dialog')).not.toBeInTheDocument()
})
