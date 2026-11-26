import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page } from 'vitest/browser'
import ProfileFileOpenerSelector from '@/pages/(main)/main/settings/nyanpasu/interface/_modules/profile-file-opener-selector'
import { m } from '@/paraglide/messages'
import { PlatformProvider } from '@nyanpasu/platform/react'
import { type ProfileFileOpener } from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { TestQueryProvider } from './query-provider'

vi.mock('@/utils/notification', () => ({ message: vi.fn() }))

test('chooses a preset and a native-dialog custom path through the dedicated command', async ({
  onTestFinished,
}) => {
  vi.stubGlobal('isTauri', true)
  let opener: ProfileFileOpener = { type: 'system_default' }
  const writes: ProfileFileOpener[] = []
  const customPath = 'C:/Tools/editor.exe'
  let picked = false
  mockIPC((command, args) => {
    const wire = args as {
      method: string
      params: { opener: ProfileFileOpener }
    }
    const method = command === 'call_rpc' ? wire.method : command
    if (method === 'get_app_config') return { profile_file_opener: opener }
    if (method === 'set_profile_file_opener') {
      opener = wire.params.opener
      writes.push(opener)
      return { status: 'committed', commits: [], value: null }
    }
    if (method === 'open_native_file_dialog') {
      picked = true
      return customPath
    }
    throw new Error(`Unexpected RPC ${method}`)
  })
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  onTestFinished(() => {
    client.clear()
    clearMocks()
    vi.unstubAllGlobals()
  })
  const selector = (clientOs: 'windows' | 'linux') => (
    <PlatformProvider
      platform={{ surface: 'desktop', clientOs }}
      bridge="tauri"
    >
      <TestQueryProvider client={client}>
        <ProfileFileOpenerSelector />
      </TestQueryProvider>
    </PlatformProvider>
  )
  const view = await render(selector('windows'))
  const trigger = view.getByRole('button', {
    name: new RegExp(m.settings_user_interface_file_opener()),
  })
  await expect.element(trigger).toBeEnabled()
  await trigger.click()
  await expect
    .element(page.getByRole('menuitemcheckbox', { name: 'Notepad++' }))
    .toBeVisible()
  await page.getByRole('menuitemcheckbox', { name: 'VS Code' }).click()
  await expect.poll(() => writes).toEqual([{ type: 'vs_code' }])
  await expect.element(view.getByText('VS Code')).toBeVisible()
  await trigger.click()
  await page
    .getByRole('menuitemcheckbox', {
      name: m.settings_user_interface_file_opener_custom(),
    })
    .click()
  await expect.poll(() => picked).toBe(true)
  await expect
    .poll(() => writes)
    .toEqual([{ type: 'vs_code' }, { type: 'custom', path: customPath }])
  await expect.element(view.getByText(customPath)).toBeVisible()

  await view.rerender(selector('linux'))
  await trigger.click()
  await expect
    .element(page.getByRole('menuitemcheckbox', { name: 'Notepad++' }))
    .not.toBeInTheDocument()
})
