import { createRoot } from 'react-dom/client'
import { expect, test, vi, type TestContext } from 'vitest'
import { m } from '@/paraglide/messages'
import type { ControllerAddress } from '@nyanpasu/query'
import { useExternalControllerForm } from '../src/pages/(main)/main/settings/web-ui/_modules/use-external-controller-form'

// The editor's error dialog. Only `apply` may open it, so a submit that never
// reaches `apply` must leave it closed too.
const dialog = vi.hoisted(() => ({ message: vi.fn() }))
vi.mock('@/utils/notification', () => dialog)

async function renderForm(
  configuredAddress: string,
  onTestFinished: TestContext['onTestFinished'],
) {
  // Stands in for the editor's IPC mutation.
  const apply = vi.fn(async (_address: ControllerAddress) => {})
  const view: { current?: ReturnType<typeof useExternalControllerForm> } = {}
  const container = document.createElement('div')

  function Form() {
    view.current = useExternalControllerForm(configuredAddress, apply)

    // The field error, read the way the editor renders it inline.
    return (
      <p>{view.current.form.formState.errors.externalController?.message}</p>
    )
  }

  const root = createRoot(container)
  root.render(<Form />)
  onTestFinished(() => {
    root.unmount()
    dialog.message.mockReset()
  })
  await expect.poll(() => view.current).toBeDefined()

  const submit = async (address: string) => {
    view.current!.form.setValue('externalController', address)
    await view.current!.handleSubmit()
  }

  return { apply, container, submit }
}

test('an address the backend cannot parse fails inline without IPC or a dialog', async ({
  onTestFinished,
}) => {
  const { apply, container, submit } = await renderForm(
    '127.0.0.1:9090',
    onTestFinished,
  )

  await submit('localhost:9090')

  await expect
    .poll(() => container.textContent)
    .toBe(m.settings_clash_settings_external_controll_invalid_address())
  expect(apply).not.toHaveBeenCalled()
  expect(dialog.message).not.toHaveBeenCalled()
})

test('a valid address reaches apply parsed', async ({ onTestFinished }) => {
  const { apply, container, submit } = await renderForm(
    '127.0.0.1:9090',
    onTestFinished,
  )

  await submit('[::1]:9091')

  expect(apply).toHaveBeenCalledExactlyOnceWith({ host: '::1', port: 9091 })
  expect(container.textContent).toBe('')
})
