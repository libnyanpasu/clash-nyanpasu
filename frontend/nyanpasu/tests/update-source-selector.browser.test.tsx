import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import UpdateSourceSelector from '../src/pages/(main)/main/settings/about/_modules/update-source-selector'

const backend = vi.hoisted(() => ({
  sources: ['nyanpasu', 'github'],
  save: vi.fn(),
  installing: false,
}))
vi.mock('@nyanpasu/query', () => ({
  useSetting: () => {
    const [value, setValue] = useState(backend.sources)
    return {
      value,
      isPending: false,
      upsert: async (sources: string[]) => {
        await backend.save(sources)
        setValue(sources)
      },
      refetch: async () => {},
    }
  },
}))
vi.mock('@/components/providers/nyanpasu-update-provider', () => ({
  useNyanpasuUpdate: () => ({
    isInstalling: backend.installing,
    isChecking: false,
  }),
}))
vi.mock('@/utils', () => ({ formatError: String }))
vi.mock('@/utils/notification', () => ({ message: vi.fn() }))

test('reorders download sources and preserves at least one enabled source', async ({
  onTestFinished,
}) => {
  backend.sources = ['nyanpasu', 'github']
  backend.installing = false
  backend.save.mockResolvedValue(undefined)
  const view = await render(<UpdateSourceSelector />)
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  await view
    .getByRole('button', {
      name: `${m.update_sources_move_up()} ${m.update_source_github()}`,
    })
    .click()
  await expect
    .poll(() => backend.save.mock.lastCall)
    .toEqual([['github', 'nyanpasu']])
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

test('locks source choices while an update is downloading or installing', async ({
  onTestFinished,
}) => {
  backend.sources = ['github']
  backend.installing = true
  const view = await render(<UpdateSourceSelector />)
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
    backend.installing = false
  })
  await expect
    .element(
      view.getByRole('switch', {
        name: m.update_source_nyanpasu(),
      }),
    )
    .toBeDisabled()
  await expect
    .element(
      view.getByRole('switch', {
        name: m.update_source_github(),
      }),
    )
    .toBeDisabled()
  expect(backend.save).not.toHaveBeenCalled()
})
