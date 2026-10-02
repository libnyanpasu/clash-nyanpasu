import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { ErrorMessage } from '@/components/error-message'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { ipcErrorMessage } from '@/utils/ipc-error'
import { message } from '@/utils/notification'
import { profileDialogLabel } from '@/utils/profile-label'
import type { IpcError, ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'

const items: ProfileItem_Serialize[] = [
  {
    uid: 't1',
    name: 'Fix rules',
    type: 'transform',
    transform: {
      type: 'script',
      runtime: 'javascript',
      source: {
        type: 'local',
        binding: { type: 'managed', file: 't1.js' },
      },
    },
  },
  {
    uid: 'p1',
    name: 'My subscription',
    type: 'config',
    config: {
      type: 'file',
      transforms: ['t1'],
      source: {
        type: 'local',
        binding: { type: 'managed', file: 'p1.yaml' },
      },
    },
  },
]
beforeEach(() => vi.stubGlobal('isTauri', true))
afterEach(() => vi.unstubAllGlobals())

const dialog = vi.hoisted(() => vi.fn().mockResolvedValue('Close'))
vi.mock('@tauri-apps/plugin-dialog', () => ({ message: dialog }))
vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({ isMinimized: async () => false }),
}))
vi.mock('@tanstack/react-router', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@tanstack/react-router')>()),
  useNavigate: () => vi.fn(),
}))
vi.mock('@nyanpasu/query', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nyanpasu/query')>()),
  useProfile: () => ({ query: { data: { items } } }),
}))

const error: IpcError = {
  kind: {
    domain: 'profiles',
    error: {
      kind: 'commit',
      source: {
        kind: 'runtime_refused',
        runtime: { kind: 'untouched' },
        errors: [
          {
            kind: 'build_runtime',
            source: {
              kind: 'transforms_failed',
              failures: [
                { kind: 'profile', id: 't1' },
                { kind: 'builtin', name: 'config_fixer' },
              ],
              logs: [
                {
                  tag: {
                    kind: 'scoped_transform',
                    data: {
                      host_profile_id: 'p1',
                      transform_profile_id: 't1',
                      role: { kind: 'selected' },
                      step_index: 0,
                      transform_kind: { type: 'script', runtime: 'javascript' },
                    },
                  },
                  entries: [
                    { level: 'log', message: 'before failure' },
                    {
                      level: 'error',
                      message: 'Error: rejected\n  at main (main.js:2)',
                    },
                  ],
                },
                {
                  tag: {
                    kind: 'builtin_transform',
                    data: {
                      name: 'config_fixer',
                      step_index: 0,
                      selected_profile_id: 'p1',
                    },
                  },
                  entries: [{ level: 'error', message: 'builtin failed' }],
                },
              ],
            },
          },
        ],
      },
    },
  },
  message: 'runtime refused',
  detail: 'original diagnostic detail',
}

afterEach(() => {
  clearMocks()
  vi.clearAllMocks()
})

test('a rejected profile returns console output and transform reports with names', () => {
  const lookup = new Map(items.map((item) => [item.uid, item]))
  const text = ipcErrorMessage(error, (id) => profileDialogLabel(lookup, id))
  expect(text).toContain('Fix rules（t1）')
  expect(text).toContain('My subscription（p1）')
  expect(text).toContain('[log] before failure')
  expect(text).toContain('[error] Error: rejected\n  at main (main.js:2)')
  expect(text).toContain('config_fixer')
  expect(text).toContain('[error] builtin failed')
  expect(text.indexOf('before failure')).toBeLessThan(
    text.indexOf('Error: rejected'),
  )
  expect(profileDialogLabel(lookup, 'deleted')).toBe('deleted')
})

test('native error dialogs resolve names without losing the caller title or logs', async () => {
  mockIPC((command, args) => {
    expect(command).toBe('call_rpc')
    expect((args as { method: string }).method).toBe('get_profiles')
    return { items, current: 'p1', global_transforms: [] }
  })
  await message(`Activation failed\n${formatError(error)}`, {
    kind: 'error',
    error,
  })
  expect(dialog.mock.calls[0][0]).toContain('Activation failed\n')
  expect(dialog.mock.calls[0][0]).toContain('Fix rules（t1）')
  expect(dialog.mock.calls[0][0]).toContain('[log] before failure')
  expect(dialog.mock.calls[0][0]).toContain('Error: rejected')
})

test('a failed name lookup still shows the original failure and logs', async () => {
  mockIPC(() => {
    throw new Error('profiles unavailable')
  })
  const text = formatError(error)
  await message(text, { kind: 'error', error })
  expect(dialog.mock.calls[0][0]).toBe(text)
})

test('profile parse failures keep the diagnostic and resolve the profile name', () => {
  const failure: IpcError = {
    ...error,
    kind: {
      domain: 'runtime',
      error: {
        kind: 'build_runtime',
        source: {
          kind: 'run_pipeline',
          source: {
            kind: 'parse_profile',
            profile: 'p1',
            message: 'invalid YAML at line 3',
          },
        },
      },
    },
  }
  const lookup = new Map(items.map((item) => [item.uid, item]))
  expect(formatError(failure, (id) => profileDialogLabel(lookup, id))).toBe(
    `${m.error_runtime_build_parse_profile({ profile: 'My subscription（p1）' })}\ninvalid YAML at line 3`,
  )
})

test('WebUI uses the chain profile tags and preserves multiline diagnostics', async ({
  onTestFinished,
}) => {
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
  })
  root.render(
    <TooltipProvider>
      <ErrorMessage error={error} />
    </TooltipProvider>,
  )
  await expect.poll(() => container.textContent).toContain('before failure')
  expect(container.textContent).toContain(
    'Error: rejected\n  at main (main.js:2)',
  )
  const tags = [...container.querySelectorAll('.bg-tertiary-container')].map(
    (tag) => tag.textContent,
  )
  expect(tags).toContain('Fix rules')
  expect(tags).toContain('My subscription')
})
