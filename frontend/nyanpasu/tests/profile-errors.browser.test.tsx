import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { ErrorMessage } from '@/components/error-message'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { ipcErrorMessage } from '@/utils/ipc-error'
import { message } from '@/utils/notification'
import { profileDialogLabel } from '@/utils/profile-label'
import { RpcFailure } from '@nyanpasu/rpc'
import type { IpcError, ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'

vi.hoisted(() => {
  Object.assign(globalThis, { isTauri: true })
})

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

const dialog = vi.hoisted(() => vi.fn())
vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({ isMinimized: async () => false }),
}))
vi.mock('@tanstack/react-router', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@tanstack/react-router')>()),
  useNavigate: () => vi.fn(),
}))
vi.mock('@nyanpasu/query', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nyanpasu/query')>()),
  useProfileListing: () => ({ data: { items } }),
}))

const domainError: IpcError = {
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
const error = new RpcFailure('activate_profile', {
  kind: 'application_error',
  message: domainError.message,
  code: 'runtime_refused',
  retryable: false,
  operation_id: 'operation-1',
  domain_error: domainError,
})

afterEach(() => {
  clearMocks()
  vi.clearAllMocks()
})

test('a rejected profile returns console output and transform reports with names', () => {
  const lookup = new Map(items.map((item) => [item.uid, item]))
  const text = ipcErrorMessage(domainError, (id) =>
    profileDialogLabel(lookup, id),
  )
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
    const { method, params } = args as { method: string; params: unknown }
    if (method === 'get_profiles_listing') {
      return { items, current: 'p1', global_transforms: [] }
    }
    expect(method).toBe('show_native_message_dialog')
    dialog((params as { message: string }).message)
    return 'Close'
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
  mockIPC((command, args) => {
    expect(command).toBe('call_rpc')
    const { method, params } = args as { method: string; params: unknown }
    if (method === 'get_profiles_listing')
      throw new Error('profiles unavailable')
    expect(method).toBe('show_native_message_dialog')
    dialog((params as { message: string }).message)
    return 'Close'
  })
  const text = formatError(error)
  await message(text, { kind: 'error', error })
  expect(dialog.mock.calls[0][0]).toBe(text)
})

test('profile parse failures keep the diagnostic and resolve the profile name', () => {
  const failure: IpcError = {
    ...domainError,
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

test.for(['desktop', 'browser'])(
  '%s diagnostic copy includes method and envelope',
  async (transport, { onTestFinished }) => {
    vi.stubGlobal('isTauri', transport === 'desktop')
    const copied = vi.fn()
    if (transport === 'desktop') {
      mockIPC((_command, args) => {
        const { method, params } = args as {
          method: string
          params: Record<string, unknown>
        }
        if (method === 'get_profiles_listing')
          return { items, current: 'p1', global_transforms: [] }
        if (method === 'show_native_message_dialog')
          return m.common_copy_error_details()
        expect(method).toBe('write_clipboard_text')
        copied(params.text)
        return null
      })
    } else {
      const alert = vi.spyOn(window, 'alert').mockImplementation(() => {})
      const confirm = vi.spyOn(window, 'confirm').mockReturnValue(true)
      const write = vi
        .spyOn(navigator.clipboard, 'writeText')
        .mockImplementation(async (text) => {
          copied(text)
        })
      onTestFinished(() => {
        alert.mockRestore()
        confirm.mockRestore()
        write.mockRestore()
      })
    }

    await message(formatError(error), { kind: 'error', error })

    expect(copied).toHaveBeenCalledOnce()
    expect(JSON.parse(copied.mock.calls[0][0])).toEqual({
      method: error.method,
      envelope: error.envelope,
    })
    expect(copied.mock.calls[0][0]).toContain(domainError.detail)
  },
)

test('invalid params keeps its message instead of localizing the domain payload', () => {
  const invalid = new RpcFailure('activate_profile', {
    kind: 'invalid_params',
    message: 'Invalid profile parameter',
    domain_error: domainError,
  })

  expect(formatError(invalid)).toBe('Invalid profile parameter')
  expect(formatError(new Error('offline'))).toBe('offline')
})

test('public IpcError without detail skips copy-details prompt and keeps localized message', async ({
  onTestFinished,
}) => {
  vi.stubGlobal('isTauri', false)
  const publicError: IpcError = {
    kind: {
      domain: 'public',
      error: {
        domain: 'system_proxy',
        error: { kind: 'write_os_proxy', enable: true, port: 7890 },
      },
    },
    message: 'system proxy operation failed',
  }
  const alert = vi.spyOn(window, 'alert').mockImplementation(() => {})
  const confirm = vi.spyOn(window, 'confirm').mockReturnValue(true)
  onTestFinished(() => {
    alert.mockRestore()
    confirm.mockRestore()
  })

  await message(formatError(publicError), {
    kind: 'error',
    error: publicError,
  })

  expect(alert).toHaveBeenCalledTimes(1)
  expect(alert).toHaveBeenCalledWith(
    m.error_public_system_proxy_write_os_proxy(),
  )
  expect(confirm).not.toHaveBeenCalled()

  await message(formatError({ ...publicError, detail: null }), {
    kind: 'error',
    error: { ...publicError, detail: null },
  })

  expect(alert).toHaveBeenCalledTimes(2)
  expect(confirm).not.toHaveBeenCalled()
})

const candidate = 'a_-012345678901234567'
const keptError = new RpcFailure('save_profile_file', {
  kind: 'application_error',
  message: 'opaque',
  code: 'runtime_refused',
  retryable: false,
  operation_id: null,
  domain_error: {
    kind: {
      domain: 'config',
      error: {
        kind: 'commit',
        domain: 'clash',
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
  },
})

test('a native error that kept a candidate offers it beside copy and close', async () => {
  const opened: unknown[] = []
  const copied: string[] = []
  let choice = m.inspect_candidate_view()
  mockIPC((command, args) => {
    expect(command).toBe('call_rpc')
    const { method, params } = args as {
      method: string
      params: { buttons: unknown; text: string }
    }
    if (method === 'open_candidate_config_window') {
      opened.push(params)
      return
    }
    if (method === 'write_clipboard_text') {
      copied.push(params.text)
      return
    }
    expect(method).toBe('show_native_message_dialog')
    expect(params.buttons).toEqual({
      type: 'yes_no_cancel_custom',
      yes: m.inspect_candidate_view(),
      no: m.common_copy_error_details(),
      cancel: m.common_close(),
    })
    return choice
  })
  const show = () => message(formatError(keptError), { error: keptError })

  await show()
  expect(opened).toEqual([{ snapshotId: candidate }])
  choice = m.common_copy_error_details()
  await show()
  expect(copied).toEqual([
    JSON.stringify(
      { method: keptError.method, envelope: keptError.envelope },
      null,
      2,
    ),
  ])
  choice = m.common_close()
  await show()
  expect(opened).toHaveLength(1)
  expect(copied).toHaveLength(1)
})

test('a message without an error offers the candidate it is given', async () => {
  const opened: unknown[] = []
  mockIPC((command, args) => {
    const { method, params } = args as {
      method: string
      params: { buttons: unknown }
    }
    if (method === 'open_candidate_config_window') {
      opened.push(params)
      return
    }
    expect(method).toBe('show_native_message_dialog')
    expect(params.buttons).toEqual({
      type: 'ok_cancel_custom',
      ok: m.inspect_candidate_view(),
      cancel: m.common_close(),
    })
    return m.inspect_candidate_view()
  })

  await message('degraded', { kind: 'warning', candidate })

  expect(opened).toEqual([{ snapshotId: candidate }])
})

test('the browser never offers a kept candidate', async ({
  onTestFinished,
}) => {
  vi.stubGlobal('isTauri', false)
  const alert = vi.spyOn(window, 'alert').mockImplementation(() => {})
  const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false)
  onTestFinished(() => {
    alert.mockRestore()
    confirm.mockRestore()
  })

  await message(formatError(keptError), { kind: 'error', error: keptError })
  await message('degraded', { kind: 'warning', candidate })

  expect(alert).toHaveBeenCalledTimes(2)
  // Only the copy-details question of the error; nothing opens a window.
  expect(confirm).toHaveBeenCalledExactlyOnceWith(
    `${m.common_copy_error_details()}?`,
  )
})
