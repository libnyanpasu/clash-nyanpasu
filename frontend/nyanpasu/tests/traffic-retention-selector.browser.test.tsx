import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import TrafficRetentionSelector from '../src/pages/(main)/main/settings/nyanpasu/_modules/traffic-retention-selector'

const backend = vi.hoisted(() => ({
  retention: '7d',
  save: vi.fn(),
}))
vi.mock('@nyanpasu/query', () => ({
  useSetting: () => {
    const [value, setValue] = useState(backend.retention)
    return {
      value,
      isPending: false,
      upsert: async (retention: string) => {
        await backend.save(retention)
        setValue(retention)
      },
    }
  },
}))

test('shows the current retention and saves the selected option', async ({
  onTestFinished,
}) => {
  backend.retention = '7d'
  backend.save.mockResolvedValue(undefined)
  const view = await render(<TrafficRetentionSelector />)
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  const trigger = view.getByRole('button', {
    name: new RegExp(m.settings_nyanpasu_traffic_retention_label()),
  })
  await expect
    .poll(() => trigger.element().textContent)
    .toContain(m.settings_nyanpasu_traffic_retention_7d())
  await expect
    .poll(() => trigger.element().textContent)
    .toContain(m.settings_nyanpasu_traffic_retention_hint())

  await trigger.click()
  await view
    .getByRole('menuitemcheckbox', {
      name: m.settings_nyanpasu_traffic_retention_30d(),
    })
    .click()

  await expect.poll(() => backend.save.mock.lastCall).toEqual(['30d'])
  await expect
    .poll(() => trigger.element().textContent)
    .toContain(m.settings_nyanpasu_traffic_retention_30d())
})

test('offers every retention option', async ({ onTestFinished }) => {
  backend.retention = '90d'
  const view = await render(<TrafficRetentionSelector />)
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  await view
    .getByRole('button', {
      name: new RegExp(m.settings_nyanpasu_traffic_retention_label()),
    })
    .click()
  await expect
    .element(
      view.getByRole('menuitemcheckbox', {
        name: m.settings_nyanpasu_traffic_retention_90d(),
      }),
    )
    .toBeChecked()
  for (const name of [
    m.settings_nyanpasu_traffic_retention_1d(),
    m.settings_nyanpasu_traffic_retention_7d(),
    m.settings_nyanpasu_traffic_retention_30d(),
    m.settings_nyanpasu_traffic_retention_forever(),
  ]) {
    await expect
      .element(view.getByRole('menuitemcheckbox', { name }))
      .toBeInTheDocument()
  }
})
