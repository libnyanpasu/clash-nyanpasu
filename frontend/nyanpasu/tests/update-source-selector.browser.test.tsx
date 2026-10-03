import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { userEvent } from 'vitest/browser'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import UpdateSourceSelector from '../src/pages/(main)/main/settings/about/_modules/update-source-selector'

const backend = vi.hoisted(() => ({
  sources: ['nyanpasu', 'github'],
  save: vi.fn(),
  busy: false,
}))
vi.mock('@nyanpasu/query', () => ({
  useSetting: () => {
    const [value, setValue] = useState(backend.sources)
    return {
      value,
      isPending: false,
      upsert: async (sources: string[]) => {
        await backend.save(sources)
        backend.sources = sources
        setValue(sources)
      },
      refetch: async () => {},
    }
  },
}))
vi.mock('@/components/providers/nyanpasu-update-provider', () => ({
  useNyanpasuUpdate: () => ({
    isBusy: backend.busy,
  }),
}))
vi.mock('@/utils', () => ({ formatError: String }))
vi.mock('@/utils/notification', () => ({ message: vi.fn() }))
vi.mock('@tauri-apps/api/core', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@tauri-apps/api/core')>()),
  isTauri: () => true,
}))

test('preserves enabled order and prevents disabling the last source', async ({
  onTestFinished,
}) => {
  backend.sources = ['nyanpasu', 'github']
  backend.busy = false
  backend.save.mockResolvedValue(undefined)
  const view = await render(
    <TooltipProvider>
      <UpdateSourceSelector />
    </TooltipProvider>,
  )
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  const proxy = view.getByRole('switch', { name: m.update_source_nyanpasu() })
  await expect.element(proxy).toBeEnabled()
  await proxy.click()
  await expect.element(proxy).not.toBeChecked()
  const github = view.getByRole('switch', { name: m.update_source_github() })
  await expect.element(github).toBeChecked()
  await expect.element(github).toBeDisabled()
  expect(backend.save.mock.lastCall).toEqual([['github']])

  await proxy.click()
  await expect.element(github).toBeEnabled()
  expect(backend.save.mock.lastCall).toEqual([['github', 'nyanpasu']])
})

test('enables GHFast, saves its priority, and restores it when reopened', async ({
  onTestFinished,
}) => {
  backend.sources = ['nyanpasu', 'github']
  backend.busy = false
  backend.save.mockResolvedValue(undefined)
  let view = await render(
    <TooltipProvider>
      <UpdateSourceSelector />
    </TooltipProvider>,
  )
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  const ghfast = view.getByRole('switch', { name: m.update_source_ghfast() })
  await expect.element(ghfast).not.toBeChecked()
  await ghfast.click()
  await expect.element(ghfast).toBeChecked()
  await expect
    .poll(() => backend.save.mock.lastCall)
    .toEqual([['nyanpasu', 'github', 'ghfast']])

  await view.unmount()
  view = await render(
    <TooltipProvider>
      <UpdateSourceSelector />
    </TooltipProvider>,
  )
  await expect
    .element(view.getByRole('switch', { name: m.update_source_ghfast() }))
    .toBeChecked()
})

test('reorders enabled sources with the keyboard drag handle', async ({
  onTestFinished,
}) => {
  backend.sources = ['nyanpasu', 'github', 'ghfast']
  backend.busy = false
  backend.save.mockResolvedValue(undefined)
  const view = await render(
    <TooltipProvider>
      <UpdateSourceSelector />
    </TooltipProvider>,
  )
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  const handleElement = view.container.querySelector(
    '[data-slot="about-package-source-item"][data-source="github"]',
  )
  if (!(handleElement instanceof HTMLElement)) {
    throw new Error('Source row is not available')
  }
  expect(backend.save).not.toHaveBeenCalled()
  handleElement.focus()
  await userEvent.keyboard('{Space}')
  expect(document.activeElement).toBe(handleElement)
  for (let index = 0; index < 12; index += 1) {
    await userEvent.keyboard('{ArrowUp}')
  }
  await userEvent.keyboard('{Space}')
  await expect
    .poll(() => backend.save.mock.lastCall)
    .toEqual([['github', 'nyanpasu', 'ghfast']])
})

test('reorders enabled sources with a pointer drag', async ({
  onTestFinished,
}) => {
  backend.sources = ['nyanpasu', 'github']
  backend.busy = false
  backend.save.mockResolvedValue(undefined)
  const view = await render(
    <TooltipProvider>
      <UpdateSourceSelector />
    </TooltipProvider>,
  )
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  const handleElement = view.container.querySelector(
    '[data-slot="about-package-source-item"][data-source="github"]',
  )
  const targetElement = view.container.querySelector(
    `[aria-label="${m.update_source_nyanpasu()}"][role="switch"]`,
  )
  if (
    !(handleElement instanceof HTMLElement) ||
    !(targetElement instanceof HTMLElement)
  ) {
    throw new Error('Source drag targets are not available')
  }
  const label = handleElement.querySelector('span')
  if (!label) throw new Error('Source label is not available')
  await userEvent.dragAndDrop(label, targetElement)
  await expect
    .poll(() => backend.save.mock.lastCall)
    .toEqual([['github', 'nyanpasu']])
  await expect
    .element(view.getByRole('switch', { name: m.update_source_nyanpasu() }))
    .toBeChecked()
  expect(
    view.container.querySelector(
      '[data-slot="about-package-source-item"][data-source="ghfast"][tabindex]',
    ),
  ).toBeNull()
})

test('cancels keyboard drag without saving or toggling a source', async ({
  onTestFinished,
}) => {
  backend.sources = ['nyanpasu', 'github']
  backend.busy = false
  backend.save.mockResolvedValue(undefined)
  const view = await render(
    <TooltipProvider>
      <UpdateSourceSelector />
    </TooltipProvider>,
  )
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  const handle = view.container.querySelector(
    '[data-slot="about-package-source-item"][data-source="github"]',
  )
  if (!(handle instanceof HTMLElement)) {
    throw new Error('Source row is not available')
  }
  handle.focus()
  await userEvent.keyboard('{Space}{ArrowUp}{Escape}')

  expect(backend.save).not.toHaveBeenCalled()
  await expect
    .element(view.getByRole('switch', { name: m.update_source_github() }))
    .toBeChecked()
})

test.for(['downloading', 'installing'])(
  'locks source choices while an update is $0',
  async (_phase, { onTestFinished }) => {
    backend.sources = ['github']
    backend.busy = true
    const view = await render(
      <TooltipProvider>
        <UpdateSourceSelector />
      </TooltipProvider>,
    )
    onTestFinished(async () => {
      await view.unmount()
      vi.clearAllMocks()
      backend.busy = false
    })
    await expect
      .element(
        view.getByRole('switch', {
          name: m.update_source_nyanpasu(),
        }),
      )
      .toBeDisabled()
    await expect
      .element(view.getByRole('switch', { name: m.update_source_ghfast() }))
      .toBeDisabled()
    await expect
      .element(
        view.getByRole('switch', {
          name: m.update_source_github(),
        }),
      )
      .toBeDisabled()
    expect(backend.save).not.toHaveBeenCalled()
  },
)
