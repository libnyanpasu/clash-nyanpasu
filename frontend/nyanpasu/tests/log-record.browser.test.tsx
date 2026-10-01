import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { LogRecord } from '../src/pages/(main)/main/logs/_modules/log-viewer'
import { m } from '../src/paraglide/messages'

const log = { type: 'info', time: '12:00:00', payload: 'dial example.com' }

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
