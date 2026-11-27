import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import {
  Dialog,
  DialogTrigger,
  FormDialogContent,
  FormDialogError,
  useDialog,
} from '@nyanpasu/ui/dialog'
import { Input } from '@nyanpasu/ui/input'

function mount(
  element: React.ReactNode,
  onTestFinished: (fn: () => void) => void,
) {
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
  })
  root.render(element)
  return container
}

test('FormDialog opens, validates, locks dismissal while submitting, and closes on success', async ({
  onTestFinished,
}) => {
  let resolveSave: (() => void) | undefined
  const onOpen = vi.fn()
  const onSaved = vi.fn()

  function NameEditor() {
    const { setOpen } = useDialog()
    const [draft, setDraft] = useState('Device A')
    const [error, setError] = useState<string | null>(null)
    const [submitting, setSubmitting] = useState(false)

    const handleSubmit = async () => {
      if (!draft.trim()) {
        setError('Name is required')
        return
      }

      setError(null)
      setSubmitting(true)
      await new Promise<void>((resolve) => {
        resolveSave = resolve
      })
      onSaved(draft.trim())
      setSubmitting(false)
      setOpen(false)
    }

    return (
      <>
        <DialogTrigger asChild>
          <button type="button" data-test="open-dialog">
            Edit name
          </button>
        </DialogTrigger>

        <FormDialogContent
          title="Device Name"
          submitLabel="Confirm"
          cancelLabel="Cancel"
          submitting={submitting}
          onOpen={() => {
            onOpen()
            setError(null)
          }}
          onSubmit={handleSubmit}
        >
          <Input
            variant="underlined"
            aria-label="Device Name"
            aria-invalid={Boolean(error)}
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
          />
          <FormDialogError message={error} />
        </FormDialogContent>
      </>
    )
  }

  const container = mount(
    <Dialog>
      <NameEditor />
    </Dialog>,
    onTestFinished,
  )

  await expect
    .poll(() => container.querySelector('[data-test="open-dialog"]'))
    .not.toBeNull()

  container
    .querySelector<HTMLButtonElement>('[data-test="open-dialog"]')!
    .click()

  await expect
    .poll(() => document.querySelector('[data-slot="dialog-surface"]'))
    .not.toBeNull()
  expect(onOpen).toHaveBeenCalledTimes(1)

  const form = document.querySelector<HTMLFormElement>(
    '[data-slot="form-dialog-form"]',
  )!
  const cancelBtn = form.querySelector<HTMLButtonElement>(
    'button[type="button"]',
  )!
  const submitBtn = form.querySelector<HTMLButtonElement>(
    'button[type="submit"]',
  )!

  // Start async submission.
  submitBtn.click()
  await expect.poll(() => submitBtn.disabled).toBe(true)
  expect(cancelBtn.disabled).toBe(true)

  // Escape must not close while submitting.
  document.dispatchEvent(
    new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }),
  )
  await new Promise((resolve) => setTimeout(resolve, 50))
  expect(document.querySelector('[data-slot="dialog-surface"]')).not.toBeNull()

  // Complete save -> dialog closes.
  resolveSave?.()
  await expect
    .poll(() => document.querySelector('[data-slot="dialog-surface"]'))
    .toBeNull()
  expect(onSaved).toHaveBeenCalledWith('Device A')
})

test('wide trigger anchors modal placeholder near the pointer click position', async ({
  onTestFinished,
}) => {
  const container = mount(
    <Dialog>
      <DialogTrigger asChild>
        <button
          type="button"
          data-test="wide-row"
          style={{ width: 600, height: 64, position: 'relative' }}
        >
          Wide list item
        </button>
      </DialogTrigger>
      <FormDialogContent
        title="Edit Row"
        submitLabel="Save"
        cancelLabel="Cancel"
        onSubmit={() => {}}
      >
        <Input variant="underlined" aria-label="Value" defaultValue="abc" />
      </FormDialogContent>
    </Dialog>,
    onTestFinished,
  )

  await expect
    .poll(() => container.querySelector('[data-test="wide-row"]'))
    .not.toBeNull()

  const row = container.querySelector<HTMLButtonElement>(
    '[data-test="wide-row"]',
  )!
  const rect = row.getBoundingClientRect()

  row.dispatchEvent(
    new PointerEvent('pointerdown', {
      bubbles: true,
      clientX: rect.left + 60,
      clientY: rect.top + 32,
    }),
  )

  await expect
    .poll(() => {
      const placeholder = row.querySelector<HTMLElement>(
        '[data-slot="modal-trigger-placeholder"]',
      )
      return placeholder ? parseFloat(placeholder.style.left) : NaN
    })
    .toBeCloseTo(60 - 24, 0)
})
