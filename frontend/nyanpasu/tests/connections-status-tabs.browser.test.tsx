import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { m } from '@/paraglide/messages'
import { StatusTabs } from '../src/pages/(main)/main/connections/_modules/status-tabs'

vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({ isMinimized: async () => false }),
}))

test('shows both counts and switches only to the other status', async ({
  onTestFinished,
}) => {
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
  })

  const onValueChange = vi.fn()
  root.render(
    <StatusTabs
      value="active"
      onValueChange={onValueChange}
      activeCount={42}
      closedCount={1024}
    />,
  )

  await expect.poll(() => container.querySelectorAll('button').length).toBe(2)
  const [active, closed] = container.querySelectorAll('button')
  expect(active.textContent).toContain(m.connections_tab_active())
  expect(active.textContent).toContain((42).toLocaleString())
  expect(closed.textContent).toContain((1024).toLocaleString())

  // Selecting the selected status again must not clear the selection.
  active.click()
  expect(onValueChange).not.toHaveBeenCalled()

  closed.click()
  expect(onValueChange).toHaveBeenCalledWith('closed')
})
