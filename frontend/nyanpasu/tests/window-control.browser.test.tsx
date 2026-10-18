import '@/assets/styles/tailwind.css'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page } from 'vitest/browser'
import { Modal, ModalContent, ModalTitle } from '@nyanpasu/ui/modal'
import WindowControl from '@/components/window/window-control'

const native = vi.hoisted(() => ({
  minimize: vi.fn(),
  toggleMaximize: vi.fn(),
  close: vi.fn(),
  setAlwaysOnTop: vi.fn(),
  upsert: vi.fn(),
}))

vi.mock('@nyanpasu/platform', () => ({ isTauri: () => true }))
vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => native,
}))
vi.mock('@nyanpasu/query', () => ({
  useSetting: () => ({ value: false, upsert: native.upsert }),
}))
vi.mock('@/hooks/use-window-maximized', () => ({
  default: () => ({
    isMaximized: false,
    toggleMaximize: native.toggleMaximize,
  }),
}))

test('window controls stay clickable above a modal without dismissing it or stealing focus', async ({
  onTestFinished,
}) => {
  const onOpenChange = vi.fn()
  const view = await render(
    <>
      <div style={{ transform: 'translateZ(0)', isolation: 'isolate' }}>
        <WindowControl />
      </div>
      <Modal defaultOpen onOpenChange={onOpenChange}>
        <ModalContent>
          <ModalTitle>Editor</ModalTitle>
          <input aria-label="Configuration" />
        </ModalContent>
      </Modal>
    </>,
  )
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  const input = page.getByRole('textbox', { name: 'Configuration' })
  await input.click()

  for (const action of ['always-on-top', 'minimize', 'maximize', 'close']) {
    const button = document.querySelector<HTMLButtonElement>(
      `[data-slot="window-control-${action}-button"]`,
    )!
    const rect = button.getBoundingClientRect()
    expect(
      button.contains(
        document.elementFromPoint(
          rect.left + rect.width / 2,
          rect.top + rect.height / 2,
        ),
      ),
    ).toBe(true)
    await page.elementLocator(button).click()
    expect(document.activeElement).toBe(input.element())
  }

  expect(native.upsert).toHaveBeenCalledWith(true)
  expect(native.setAlwaysOnTop).toHaveBeenCalledWith(true)
  expect(native.minimize).toHaveBeenCalledOnce()
  expect(native.toggleMaximize).toHaveBeenCalledOnce()
  expect(native.close).toHaveBeenCalledOnce()
  expect(onOpenChange).not.toHaveBeenCalled()
  await expect.element(page.getByRole('dialog')).toBeVisible()
})
