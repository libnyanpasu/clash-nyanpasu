// Tailwind sizes the scroll area, so the table has rows out of view.
import '@/assets/styles/tailwind.css'
import { useState } from 'react'
import { flushSync } from 'react-dom'
import { createRoot } from 'react-dom/client'
import { expect, test } from 'vitest'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
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
    .poll(() => container.querySelector('tbody tr')?.textContent)
    .toBe('row-1')
  await new Promise((resolve) => setTimeout(resolve, 100))
  expect(focused()).toBeNull()
})
