import '@nyanpasu/theme/styles/theme.css'
import '@/assets/styles/tailwind.css'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page, userEvent } from 'vitest/browser'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import { Route as EditorPage } from '@/pages/(editor)/editor/profile/index'
import { m } from '@/paraglide/messages'
import type { IpcError, ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import { clearMocks, mockIPC, mockWindows } from '@tauri-apps/api/mocks'
import { TestQueryProvider } from './query-provider'

vi.hoisted(() => vi.stubGlobal('isTauri', true))
beforeEach(() => vi.stubGlobal('isTauri', true))
afterEach(() => vi.unstubAllGlobals())
vi.mock('@monaco-editor/react', () => ({
  default: ({
    value,
    onChange,
  }: {
    value?: string
    onChange?: (value: string) => void
  }) => (
    <textarea
      aria-label="profile content"
      value={value ?? ''}
      onChange={(event) => onChange?.(event.target.value)}
    />
  ),
}))
// Monaco and its workers are not under test here.
vi.mock('@/services/monaco', () => ({}))
vi.mock('@/pages/(editor)/editor/_modules/use-monaco-theme', () => ({
  useMonacoTheme: () => 'test',
}))
vi.mock('@/components/window/window-control', () => ({
  default: () => <div className="h-8 w-35 shrink-0" aria-hidden />,
}))
vi.mock('@/components/logo/animated-logo', () => ({ default: () => null }))

const candidate = 'a_-012345678901234567'
const profile: ProfileItem_Serialize = {
  uid: 'p1',
  name: 'Local',
  type: 'config',
  config: {
    type: 'file',
    transforms: [],
    source: { type: 'local', binding: { type: 'managed', file: 'p1.yaml' } },
  },
}
const refused: IpcError = {
  kind: {
    domain: 'profiles',
    error: {
      kind: 'commit',
      uid: 'p1',
      path: 'p1.yaml',
      source: {
        kind: 'runtime_refused',
        errors: [
          {
            kind: 'core_rejected_config',
            core_kind: 'config_check_failed',
            message: 'opaque',
            candidate_snapshot_id: candidate,
          },
        ],
      },
    },
  },
  message: 'opaque',
  detail: 'opaque',
}

// The desktop transport rejects with the RPC envelope that carries the domain error.
const envelope = {
  kind: 'application_error',
  message: 'opaque',
  domain_error: refused,
}

test('a refused editor save shows one dialog that offers the kept candidate', async ({
  onTestFinished,
}) => {
  const dialogs: { buttons: unknown }[] = []
  const opened: unknown[] = []
  mockIPC(
    (command, args) => {
      expect(command).toBe('call_rpc')
      const { method, params } = args as {
        method: string
        params: Record<string, unknown>
      }
      switch (method) {
        case 'get_profiles':
          return { current: [], items: [profile], folders: [] }
        case 'get_profiles_listing':
          return { items: [profile], current: [], global_transforms: [] }
        case 'read_profile_file':
          return 'mode: rule'
        case 'save_profile_file':
          return Promise.reject(envelope)
        case 'show_native_message_dialog':
          dialogs.push(params as { buttons: unknown })
          return m.inspect_candidate_view()
        case 'open_candidate_config_window':
          opened.push(params)
          return
      }
      throw new Error(`Unexpected RPC ${method}`)
    },
    { shouldMockEvents: true },
  )
  mockWindows('editor-p1')
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  const root = createRootRoute({
    component: () => (
      <TestQueryProvider client={client}>
        <BlockTaskProvider>
          <TooltipProvider>
            <Outlet />
          </TooltipProvider>
        </BlockTaskProvider>
      </TestQueryProvider>
    ),
  })
  const editor = EditorPage.update({
    id: '/editor/profile/',
    path: '/editor/profile',
    getParentRoute: () => root,
  } as never)
  const router = createRouter({
    routeTree: root.addChildren([editor]),
    history: createMemoryHistory({
      initialEntries: ['/editor/profile?uid=p1'],
    }),
  })
  const view = await render(<RouterProvider router={router as never} />)
  onTestFinished(async () => {
    await view.unmount()
    client.clear()
    clearMocks()
  })

  const content = page.getByRole('textbox', { name: 'profile content' })
  await expect.element(content).toHaveValue('mode: rule')
  await userEvent.fill(content, 'mode: global')
  await userEvent.click(page.getByRole('button', { name: m.common_apply() }))

  await expect.poll(() => opened).toEqual([{ snapshotId: candidate }])
  expect(dialogs).toHaveLength(1)
  expect(dialogs[0].buttons).toEqual({
    type: 'yes_no_cancel_custom',
    yes: m.inspect_candidate_view(),
    no: m.common_copy_error_details(),
    cancel: m.common_close(),
  })
  // The draft stays for another try.
  await expect.element(content).toHaveValue('mode: global')
})
