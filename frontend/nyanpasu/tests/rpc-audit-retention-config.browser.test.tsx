import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { userEvent } from 'vitest/browser'
import { m } from '@/paraglide/messages'
import type { RpcAuditSettings } from '@nyanpasu/rpc/types'
import RpcAuditRetentionConfig from '../src/pages/(main)/main/settings/nyanpasu/logs/_modules/rpc-audit-retention-config'

const backend = vi.hoisted(() => ({
  settings: undefined as RpcAuditSettings | undefined,
  save: vi.fn(),
}))
vi.mock('@nyanpasu/query', () => ({
  useSetting: () => {
    const [value, setValue] = useState(backend.settings)
    return {
      value,
      upsert: async (settings: RpcAuditSettings) => {
        await backend.save(settings)
        setValue(settings)
      },
    }
  },
}))

test('each slider saves the whole settings object', async ({
  onTestFinished,
}) => {
  backend.settings = { max_age_days: 30, max_size_mib: 64 }
  backend.save.mockResolvedValue(undefined)
  const view = await render(<RpcAuditRetentionConfig />)
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  view
    .getByRole('slider', { name: m.settings_logs_rpc_audit_max_age_label() })
    .element()
    .focus()
  await userEvent.keyboard('{ArrowRight}')
  await expect
    .poll(() => backend.save.mock.lastCall)
    .toEqual([{ max_age_days: 31, max_size_mib: 64 }])

  view
    .getByRole('slider', { name: m.settings_logs_rpc_audit_max_size_label() })
    .element()
    .focus()
  await userEvent.keyboard('{End}')
  await expect
    .poll(() => backend.save.mock.lastCall)
    .toEqual([{ max_age_days: 31, max_size_mib: 256 }])
  await expect
    .element(view.getByText(m.settings_logs_rpc_audit_hint()))
    .toBeInTheDocument()
})
