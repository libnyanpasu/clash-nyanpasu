import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { LogRecord } from '../src/pages/(main)/main/logs/_modules/log-viewer'
import { m } from '../src/paraglide/messages'

const log = { type: 'info', time: '12:00:00', payload: 'dial example.com' }

test('a Core preview fetches complete text for copy and closing an in-flight detail releases the row', async ({
  onTestFinished,
}) => {
  const writeText = vi
    .spyOn(navigator.clipboard, 'writeText')
    .mockResolvedValue(undefined)
  onTestFinished(() => writeText.mockRestore())
  let finishPending!: (value: unknown) => void
  const loadRaw = vi
    .fn()
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishPending = resolve
        }),
    )
    .mockResolvedValue({ ...log, payload: 'complete body' })
  const props = {
    time: log.time,
    level: log.type,
    message: 'preview',
    raw: log,
    search: '',
    loadRaw,
    onInspect: vi.fn(),
  }
  const view = await render(<LogRecord {...props} expanded />)
  onTestFinished(() => view.unmount())
  await expect.poll(() => loadRaw.mock.calls.length).toBe(1)
  await view.rerender(<LogRecord {...props} expanded={false} />)
  await expect
    .element(view.getByRole('button', { name: m.logs_copy() }))
    .toBeEnabled()
  finishPending({ ...log, payload: 'stale detail' })
  await view.getByRole('button', { name: m.logs_copy() }).click()
  expect(writeText).toHaveBeenCalledWith(
    JSON.stringify({ ...log, payload: 'complete body' }, null, 2),
  )
  expect(view.container.querySelector('[role="region"]')).toBeNull()
})

test('a record row copies and inspects its record as JSON', async ({
  onTestFinished,
}) => {
  const writeText = vi
    .spyOn(navigator.clipboard, 'writeText')
    .mockResolvedValue(undefined)
  onTestFinished(() => writeText.mockRestore())
  const onInspect = vi.fn()
  const view = await render(
    <LogRecord
      time={log.time}
      level={log.type}
      message={log.payload}
      raw={log}
      search=""
      onInspect={onInspect}
    />,
  )
  onTestFinished(() => view.unmount())

  await view.getByRole('button', { name: m.logs_copy() }).click()
  expect(writeText).toHaveBeenCalledWith(JSON.stringify(log, null, 2))
  await expect
    .element(view.getByRole('button', { name: m.logs_copied() }))
    .toBeVisible()

  const inspect = view.getByRole('button', { name: m.logs_view_json() })
  await inspect.click()
  expect(onInspect).toHaveBeenCalledOnce()
  await expect.element(inspect).toHaveAttribute('aria-expanded', 'true')
  await expect
    .poll(() => view.container.querySelector('[role="region"]')?.textContent)
    .toMatch(/"payload":\s*"dial example\.com"/)
})

test('a text record copies its raw line unchanged', async ({
  onTestFinished,
}) => {
  const writeText = vi
    .spyOn(navigator.clipboard, 'writeText')
    .mockResolvedValue(undefined)
  onTestFinished(() => writeText.mockRestore())
  const view = await render(
    <LogRecord
      time="—"
      level="warn"
      message="disk full"
      raw='{"message":"disk full"}'
      search=""
      onInspect={() => {}}
    />,
  )
  onTestFinished(() => view.unmount())

  await view.getByRole('button', { name: m.logs_copy() }).click()
  expect(writeText).toHaveBeenCalledWith('{"message":"disk full"}')
})

test('the copy button returns to copy after confirming', async ({
  onTestFinished,
}) => {
  const writeText = vi
    .spyOn(navigator.clipboard, 'writeText')
    .mockResolvedValue(undefined)
  onTestFinished(() => writeText.mockRestore())
  const view = await render(
    <LogRecord
      time={log.time}
      level={log.type}
      message={log.payload}
      raw={log}
      search=""
      onInspect={() => {}}
    />,
  )
  onTestFinished(() => view.unmount())

  await view.getByRole('button', { name: m.logs_copy() }).click()
  await expect
    .element(view.getByRole('button', { name: m.logs_copied() }))
    .toBeVisible()
  await expect
    .element(view.getByRole('button', { name: m.logs_copy() }), {
      timeout: 4000,
    })
    .toBeVisible()
})
