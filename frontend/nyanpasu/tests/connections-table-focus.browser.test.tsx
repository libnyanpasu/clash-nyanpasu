// Tailwind sizes the scroll area, so the table has rows out of view.
import '@/assets/styles/tailwind.css'
import { useState } from 'react'
import { flushSync } from 'react-dom'
import { createRoot } from 'react-dom/client'
import { expect, test } from 'vitest'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import {
  DataPage,
  DataPageSurface,
  DataPageToolbar,
} from '../src/pages/(main)/main/_modules/data-page'
import ConnectionsTable, {
  type ConnectionColumn,
} from '../src/pages/(main)/main/connections/_modules/connections-table'
import { useTabFocus } from '../src/pages/(main)/main/connections/_modules/use-connection-rows'

type Row = { id: string }

const rows: Row[] = Array.from({ length: 300 }, (_, index) => ({
  id: `row-${index + 1}`,
}))

const columns: Array<ConnectionColumn<Row>> = [
  {
    id: 'Host',
    header: () => 'Host',
    accessorFn: ({ id }) => id,
    cell: (info) => info.row.original.id,
  },
]

const rowId = (row: Row) => row.id

test('the focused row scrolls into view and is highlighted', async ({
  onTestFinished,
}) => {
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
    localStorage.clear()
  })

  root.render(
    <ScrollArea className="h-96">
      <ConnectionsTable
        settingsKey="connections-columns-focus-test"
        columns={columns}
        data={rows}
        getRowId={rowId}
        emptyMessage=""
        focusRowId="row-150"
        settingsOpen={false}
        onSettingsOpenChange={() => {}}
      />
    </ScrollArea>,
  )

  const viewport = () =>
    container.querySelector<HTMLElement>('[data-slot="scroll-area-viewport"]')!

  await expect.poll(() => viewport().scrollTop).toBeGreaterThan(0)
  await expect
    .poll(() => container.querySelector('tr[data-focused="true"]')?.textContent)
    .toBe('row-150')

  // The highlight fades slowly; hover and press feedback stay quick.
  const focused = container.querySelector('tr[data-focused="true"]')!
  const other = container.querySelector('tr[data-focused="false"]')!

  expect(getComputedStyle(focused).animationName).toBe('focus-highlight')
  expect(getComputedStyle(focused).animationDuration).toBe('2s')
  expect(getComputedStyle(other).animationName).toBe('none')
  expect(getComputedStyle(other).transitionDuration).toBe('0.15s')
})

// The page's tables, keyed by tab as the page keys its scroll area.
function Tabs({
  focus,
  onScopeChange,
}: {
  focus: string
  onScopeChange: (setScope: (scope: string) => void) => void
}) {
  const [scope, setScope] = useState('active')

  onScopeChange(setScope)

  const focusRowId = useTabFocus(scope, focus)

  return (
    <ScrollArea key={scope} className="h-96">
      <ConnectionsTable
        settingsKey="connections-columns-focus-test"
        columns={columns}
        data={rows}
        getRowId={rowId}
        emptyMessage=""
        focusRowId={focusRowId}
        settingsOpen={false}
        onSettingsOpenChange={() => {}}
      />
    </ScrollArea>
  )
}

test('switching tabs and back does not focus the row again', async ({
  onTestFinished,
}) => {
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
    localStorage.clear()
  })

  let setScope: (scope: string) => void = () => {}

  root.render(
    <Tabs
      focus="row-150"
      onScopeChange={(next) => {
        setScope = next
      }}
    />,
  )

  const focused = () => container.querySelector('tr[data-focused="true"]')

  await expect.poll(() => focused()?.textContent).toBe('row-150')

  flushSync(() => setScope('closed'))
  flushSync(() => setScope('active'))

  // The returned-to table starts at the top, with nothing highlighted.
  await expect
    .poll(
      () => container.querySelector('tbody tr:not([aria-hidden])')?.textContent,
    )
    .toBe('row-1')
  await new Promise((resolve) => setTimeout(resolve, 100))
  expect(focused()).toBeNull()
})

test('the last connection scrolls clear of the dock after it wraps', async ({
  onTestFinished,
}) => {
  const container = document.createElement('div')
  container.style.cssText = 'display:flex;width:700px;height:500px'
  document.body.append(container)
  const root = createRoot(container)
  let clicked: string | undefined
  onTestFinished(() => {
    root.unmount()
    container.remove()
    localStorage.clear()
  })

  const content = (
    <DataPage
      title="Connections"
      toolbar={
        <DataPageToolbar>
          <button className="h-12 w-48 shrink-0">Search</button>
          <button className="h-12 w-48 shrink-0">Tabs</button>
        </DataPageToolbar>
      }
    >
      <DataPageSurface>
        <ScrollArea className="min-h-0 flex-1" scrollbars="both" type="always">
          <div className="min-w-[900px]">
            <ConnectionsTable
              settingsKey="connections-columns-dock-test"
              columns={columns}
              data={rows}
              getRowId={rowId}
              renderRow={(row, props) => (
                <tr
                  {...props}
                  onClick={() => {
                    clicked = row.id
                  }}
                />
              )}
              emptyMessage=""
              settingsOpen={false}
              onSettingsOpenChange={() => {}}
            />
          </div>
        </ScrollArea>
      </DataPageSurface>
    </DataPage>
  )
  root.render(content)

  const viewport = () =>
    container.querySelector<HTMLElement>('[data-slot="scroll-area-viewport"]')!
  const header = () => container.querySelector<HTMLElement>('thead th')!
  const dock = () =>
    container.querySelector<HTMLElement>('[data-slot="data-page-dock"]')!
  const row = (id: string) =>
    [
      ...container.querySelectorAll<HTMLElement>('tbody tr:not([aria-hidden])'),
    ].find((candidate) => candidate.textContent === id)!
  await expect.poll(() => viewport()?.scrollHeight ?? 0).toBeGreaterThan(500)
  for (const width of [700, 260, 700]) {
    if (width === 260) {
      // A new page mount must use the same geometry as an existing page.
      root.render(null)
      await expect.poll(() => container.childElementCount).toBe(0)
      root.render(content)
      await expect.poll(() => dock()?.offsetHeight ?? 0).toBeGreaterThan(0)
    }
    container.style.width = `${width}px`
    await expect.poll(() => dock().offsetHeight).toBe(width === 700 ? 48 : 104)
    const horizontal = container.querySelector<HTMLElement>(
      '[data-slot="scroll-area-scrollbar"][data-orientation="horizontal"]',
    )!
    const vertical = container.querySelector<HTMLElement>(
      '[data-slot="scroll-area-scrollbar"][data-orientation="vertical"]',
    )!
    await expect
      .poll(
        () =>
          dock().getBoundingClientRect().top -
          (horizontal?.getBoundingClientRect().bottom ?? Infinity),
      )
      .toBeCloseTo(8, 0)
    expect(horizontal.getBoundingClientRect().left).toBeCloseTo(
      viewport().getBoundingClientRect().left + 12,
      0,
    )
    await expect
      .poll(() => vertical?.getBoundingClientRect().bottom ?? Infinity)
      .toBeLessThanOrEqual(horizontal.getBoundingClientRect().top - 8)

    viewport().scrollTop = 44 * 149
    await expect.poll(() => row('row-150')?.textContent).toBe('row-150')
    const viewportBox = viewport().getBoundingClientRect()
    const headerBox = header().getBoundingClientRect()
    const middleRow = row('row-150')
    expect(headerBox.top).toBe(viewportBox.top)
    expect(middleRow.getBoundingClientRect().top).toBeCloseTo(
      headerBox.bottom,
      0,
    )
    expect(middleRow.getBoundingClientRect().bottom).toBeLessThan(
      dock().getBoundingClientRect().top,
    )
    expect(
      middleRow.firstElementChild!.getBoundingClientRect().width,
    ).toBeCloseTo(header().getBoundingClientRect().width, 0)
    middleRow.click()
    expect(clicked).toBe('row-150')

    await expect
      .poll(() => {
        viewport().scrollTop = viewport().scrollHeight
        return row('row-300')?.getBoundingClientRect().bottom ?? Infinity
      })
      .toBeLessThanOrEqual(dock().getBoundingClientRect().top)
  }
})

test('vertical tracks reserve horizontal clearance only when that track exists', async ({
  onTestFinished,
}) => {
  const container = document.createElement('div')
  container.style.cssText = 'display:flex;width:700px;height:500px'
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
  })

  for (const scrollbars of ['vertical', 'both', 'vertical'] as const) {
    root.render(null)
    await expect.poll(() => container.childElementCount).toBe(0)
    root.render(
      <DataPage
        title="Tracks"
        toolbar={
          <DataPageToolbar>
            <button className="h-11">Search</button>
          </DataPageToolbar>
        }
      >
        <DataPageSurface>
          <ScrollArea
            className="min-h-0 flex-1"
            scrollbars={scrollbars}
            type="always"
          >
            <div className="h-[1000px] min-w-[900px]" />
          </ScrollArea>
        </DataPageSurface>
      </DataPage>,
    )
    await expect
      .poll(() => container.querySelector('[data-orientation="vertical"]'))
      .not.toBeNull()
    const vertical = container.querySelector<HTMLElement>(
      '[data-slot="scroll-area-scrollbar"][data-orientation="vertical"]',
    )!
    const dock = container.querySelector<HTMLElement>(
      '[data-slot="data-page-dock"]',
    )!
    await expect
      .poll(
        () =>
          dock.getBoundingClientRect().top -
          vertical.getBoundingClientRect().bottom,
      )
      .toBeCloseTo(scrollbars === 'both' ? 32 : 8, 0)
  }
})
