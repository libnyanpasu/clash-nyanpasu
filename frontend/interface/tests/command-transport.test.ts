import { afterEach, describe, expect, it, vi } from 'vitest'
import { invokeHttpCommand } from '../src/ipc/command-transport'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

afterEach(() => {
  vi.unstubAllGlobals()
  invoke.mockReset()
  vi.resetModules()
})

describe('rpc command facade', () => {
  it('uses Tauri IPC for generated commands in the desktop app', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    invoke.mockResolvedValue({ items: [] })

    const { rpc } = await import('../src/ipc/rpc')
    await expect(rpc.getProfiles()).resolves.toEqual({
      status: 'ok',
      data: { items: [] },
    })
    expect(invoke).toHaveBeenCalledWith('call_rpc', {
      method: 'get_profiles',
      params: {},
    })
  })

  it('uses the same command name and camelCase arguments over HTTP', async () => {
    vi.stubGlobal('window', {})
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ items: [] }),
    })
    vi.stubGlobal('fetch', fetchMock)

    const { rpc } = await import('../src/ipc/rpc')
    await expect(rpc.inspectRuntimeNode('a', 2)).resolves.toEqual({
      status: 'ok',
      data: { items: [] },
    })
    expect(fetchMock).toHaveBeenCalledWith('/bridge/rpc', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        method: 'inspect_runtime_node',
        params: { snapshotId: 'a', nodeId: 2 },
      }),
    })
  })

  it('reports unsupported commands from the experimental HTTP registry', async () => {
    vi.stubGlobal('window', {})
    const fetchMock = vi.fn().mockResolvedValue({
      ok: false,
      status: 501,
      json: async () => ({
        kind: 'unsupported',
        message:
          'command `quit_application` is unavailable over experimental HTTP',
      }),
    })
    vi.stubGlobal('fetch', fetchMock)

    await expect(invokeHttpCommand('quit_application')).rejects.toEqual({
      kind: 'unsupported',
      message:
        'command `quit_application` is unavailable over experimental HTTP',
    })
    expect(fetchMock).toHaveBeenCalledOnce()
  })

  it('preserves RPC error objects and rejects transport failures', async () => {
    vi.stubGlobal('window', {})
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({
        ok: false,
        status: 400,
        json: async () => ({ kind: 'invalid_params', message: 'Bad input' }),
      })
      .mockResolvedValueOnce({
        ok: false,
        status: 502,
        json: async () => {
          throw new SyntaxError()
        },
      })
    vi.stubGlobal('fetch', fetchMock)

    const { rpc } = await import('../src/ipc/rpc')
    await expect(rpc.getProfiles()).resolves.toEqual({
      status: 'error',
      error: { kind: 'invalid_params', message: 'Bad input' },
    })
    await expect(rpc.getProfiles()).rejects.toThrow(
      'RPC returned a non-JSON response (HTTP 502)',
    )
  })
  it.each(['desktop', 'http'])(
    'preserves operation identity and log error codes over %s',
    async (transport) => {
      vi.stubGlobal(
        'window',
        transport === 'desktop' ? { __TAURI_INTERNALS__: {} } : {},
      )
      const coreError = {
        kind: 'application_error',
        message: 'pending',
        code: 'backend_unavailable',
        retryable: false,
        operation_id: 'operation-1',
        domain_error: null,
      }
      const logError = {
        kind: 'application_error',
        message: 'log session expired',
        domain_error: 'session_expired',
      }
      if (transport === 'desktop') {
        invoke.mockRejectedValueOnce(coreError).mockRejectedValueOnce(logError)
      } else {
        vi.stubGlobal(
          'fetch',
          vi
            .fn()
            .mockResolvedValueOnce({ ok: false, json: async () => coreError })
            .mockResolvedValueOnce({ ok: false, json: async () => logError }),
        )
      }
      const { rpc } = await import('../src/ipc/rpc')
      await expect(rpc.restartSidecar()).resolves.toEqual({
        status: 'error',
        error: coreError,
      })
      await expect(rpc.listLogFiles('app')).resolves.toEqual({
        status: 'error',
        error: 'session_expired',
      })
    },
  )
})
