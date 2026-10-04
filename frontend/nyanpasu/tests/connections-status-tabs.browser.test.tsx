import type { ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { m } from '@/paraglide/messages'
import { StatusTabs } from '../src/pages/(main)/main/connections/_modules/status-tabs'

vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({ isMinimized: async () => false }),
}))

function render(node: ReactNode, onTestFinished: (fn: () => void) => void) {
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
  })
  root.render(node)
  return container
}

test('shows the scopes in traffic order and switches only to another scope', async ({
  onTestFinished,
}) => {
  const onValueChange = vi.fn()
  const container = render(
    <StatusTabs
      value="active"
      onValueChange={onValueChange}
      activeCount={42}
      closedCount={1024}
    />,
    onTestFinished,
  )

  await expect.poll(() => container.querySelectorAll('button').length).toBe(3)
  const [all, active, closed] = container.querySelectorAll('button')
  // All needs no count: it is the sum of the other two.
  expect(all.textContent).toBe(m.connections_tab_all())
  expect(active.textContent).toContain(m.connections_tab_active())
  expect(active.textContent).toContain((42).toLocaleString())
  expect(closed.textContent).toContain(m.connections_tab_closed())
  expect(closed.textContent).toContain((1024).toLocaleString())

  // Selecting the selected scope again must not clear the selection.
  active.click()
  expect(onValueChange).not.toHaveBeenCalled()

  closed.click()
  expect(onValueChange).toHaveBeenCalledWith('closed')

  all.click()
  expect(onValueChange).toHaveBeenCalledWith('all')
})
