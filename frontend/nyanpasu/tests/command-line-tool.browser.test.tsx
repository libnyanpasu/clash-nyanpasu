import { expect, test, vi, type TestContext } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import type { PlatformInfo } from '@nyanpasu/platform'
import { PlatformProvider } from '@nyanpasu/platform/react'
import { QueryClient } from '@tanstack/react-query'
import {
  createTestRpc,
  rpcWrapper,
  type RpcHandler,
} from '../../query/tests/rpc-test-utils'
import CommandLineTool from '../src/pages/(main)/main/settings/nyanpasu/remote-access/_modules/command-line-tool'

const dialogs = vi.hoisted(() => ({
  message: vi.fn(async () => undefined),
  copy: vi.fn(async () => undefined),
}))
vi.mock('@/utils/notification', () => ({
  ask: vi.fn(async () => true),
  message: dialogs.message,
}))
vi.mock('@/utils/clipboard', () => ({ writeClipboardText: dialogs.copy }))

const installed = {
  link: '/home/u/.local/bin/nyanpasu-cli',
  target: '/Applications/Clash Nyanpasu.app/Contents/MacOS/nyanpasu-cli',
  already_linked: false,
  on_path: true,
  system_command:
    "sudo ln -sf '/Applications/Clash Nyanpasu.app/Contents/MacOS/nyanpasu-cli' /usr/local/bin/nyanpasu-cli",
}

async function mount(
  onTestFinished: TestContext['onTestFinished'],
  platform: PlatformInfo,
  handlers: Record<string, RpcHandler> = {},
  appImage = false,
) {
  const testRpc = createTestRpc({
    is_appimage: () => appImage,
    install_cli: () => installed,
    ...handlers,
  })
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  const Wrapper = rpcWrapper(testRpc.rpc, client)
  const view = await render(
    <PlatformProvider platform={platform}>
      <Wrapper>
        <CommandLineTool />
      </Wrapper>
    </PlatformProvider>,
  )
  onTestFinished(async () => {
    await view.unmount()
    client.clear()
    testRpc.rpc.dispose()
    vi.clearAllMocks()
  })

  return { view, testRpc }
}

const macos: PlatformInfo = { surface: 'desktop', clientOs: 'macos' }
const linux: PlatformInfo = { surface: 'desktop', clientOs: 'linux' }

const installButton = (view: Awaited<ReturnType<typeof render>>) =>
  view.getByRole('button', { name: m.settings_remote_access_cli_install() })

test('the macOS app links the tool and offers the system-wide command', async ({
  onTestFinished,
}) => {
  const { view, testRpc } = await mount(onTestFinished, macos)

  await installButton(view).click()

  await expect
    .element(
      view.getByText(
        m.settings_remote_access_cli_installed({ link: installed.link }),
      ),
    )
    .toBeVisible()
  expect(testRpc.invoke).toHaveBeenCalledWith('install_cli', undefined)
  await expect.element(view.getByText(installed.system_command)).toBeVisible()
  expect(
    view.container.querySelector('[data-slot="remote-access-cli-not-on-path"]'),
  ).toBeNull()

  await view.getByRole('button', { name: m.common_copy() }).click()
  expect(dialogs.copy).toHaveBeenCalledWith(installed.system_command)
})

test('a folder outside PATH is explained', async ({ onTestFinished }) => {
  const { view } = await mount(onTestFinished, macos, {
    install_cli: () => ({ ...installed, on_path: false }),
  })

  await installButton(view).click()

  await expect
    .element(
      view.getByText(
        m.settings_remote_access_cli_not_on_path({ link: installed.link }),
      ),
    )
    .toBeVisible()
})

test('an AppImage says it copies the tool out of the image first', async ({
  onTestFinished,
}) => {
  const { view } = await mount(onTestFinished, linux, {}, true)

  await expect
    .element(view.getByText(m.settings_remote_access_cli_appimage()))
    .toBeVisible()
  await expect.element(installButton(view)).toBeVisible()
})

test('a refusal is shown and nothing claims success', async ({
  onTestFinished,
}) => {
  const { view } = await mount(onTestFinished, macos, {
    install_cli: () => {
      throw new Error('already exists and is not a link')
    },
  })

  await installButton(view).click()

  await expect.element(view.getByRole('alert')).toBeVisible()
  expect(view.container.textContent).not.toContain(installed.link)
})

test('packages and installers already provide the tool, so nothing is offered', async ({
  onTestFinished,
}) => {
  const { view } = await mount(onTestFinished, linux)

  // Let the AppImage query settle: a plain Linux install shows nothing.
  await vi.waitFor(() =>
    expect(
      view.container.querySelector('[data-slot="remote-access-cli-container"]'),
    ).toBeNull(),
  )
  expect(view.container.textContent).toBe('')
})
