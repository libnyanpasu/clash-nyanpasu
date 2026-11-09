import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import TransparentProxyButton from '../src/pages/(main)/main/settings/system/proxy/_modules/transparent-proxy-button'

const backend = vi.hoisted(() => ({
  values: {} as Record<string, unknown>,
  supported: true,
  actualActive: false,
  actualMode: null as string | null,
  upsert: vi.fn(),
  invalidate: vi.fn(),
}))

vi.mock('@nyanpasu/platform', () => ({ isLinux: true }))
vi.mock('@nyanpasu/query', () => ({
  useSetting: (key: string) => ({ value: backend.values[key] }),
  useClashSetting: (key: string) => {
    const [value, setValue] = useState(backend.values[key])
    return {
      value,
      upsert: async (next: unknown) => {
        await backend.upsert(key, next)
        setValue(next)
      },
    }
  },
  useQueryApi: () => ({
    queries: {
      getTransparentProxyStatus: () => ({
        queryKey: ['getTransparentProxyStatus'],
        queryFn: () => Promise.resolve({ supported: backend.supported }),
      }),
    },
  }),
  unwrapQueryOptions: (options: unknown) => options,
}))
vi.mock('@tanstack/react-query', () => ({
  useQuery: () => ({
    data: {
      supported: backend.supported,
      active: backend.actualActive,
      mode: backend.actualMode,
      error: null,
    },
    isError: false,
    isPending: false,
  }),
  useQueryClient: () => ({ invalidateQueries: backend.invalidate }),
}))
vi.mock('@tanstack/react-router', () => ({
  Link: ({ children }: { children: React.ReactNode }) => (
    <a href="/network">{children}</a>
  ),
}))
vi.mock('@/components/providers/block-task-provider', () => ({
  useBlockTask: (_key: string, execute: () => Promise<void>) => ({
    execute,
    isPending: false,
  }),
}))
vi.mock('@/components/settings/system-proxy', () => ({
  ProxyButton: ({
    children,
    isActive,
    ...props
  }: React.ButtonHTMLAttributes<HTMLButtonElement> & { isActive: boolean }) => (
    <button {...props} data-active={String(isActive)}>
      {children}
    </button>
  ),
}))
vi.mock('@/utils/notification', () => ({ message: vi.fn() }))

async function setup(
  onTestFinished: (fn: () => Promise<void>) => void,
  overrides: Record<string, unknown> = {},
) {
  backend.values = {
    core: 'mihomo',
    enable_service_mode: true,
    enable_tun_mode: false,
    tproxy_port: null,
    transparent_proxy: {
      mode: 'disabled',
      local: false,
      interfaces: ['eth0'],
      ipv6: true,
    },
    ...overrides,
  }
  backend.supported = true
  backend.actualActive = false
  backend.actualMode = null
  backend.upsert.mockResolvedValue(undefined)
  backend.invalidate.mockResolvedValue(undefined)
  const view = await render(<TransparentProxyButton />)
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })
  return view
}

test('enables TPROXY and disables capture while preserving its scope', async ({
  onTestFinished,
}) => {
  const view = await setup(onTestFinished)
  const button = view.getByRole('button', {
    name: m.settings_clash_transparent_proxy_label(),
    exact: false,
  })
  await button.click()
  await expect.poll(() => backend.upsert.mock.calls.length).toBe(2)
  expect(backend.upsert.mock.calls).toEqual([
    ['tproxy_port', { kind: 'fixed', start_port: 7894 }],
    [
      'transparent_proxy',
      { mode: 'tproxy', local: false, interfaces: ['eth0'], ipv6: true },
    ],
  ])
  await expect.element(button).toHaveAttribute('data-active', 'true')
  await button.click()
  await expect.poll(() => backend.upsert.mock.calls.length).toBe(3)
  expect(backend.upsert.mock.calls[2]).toEqual([
    'transparent_proxy',
    { mode: 'disabled', local: false, interfaces: ['eth0'], ipv6: true },
  ])
})

test('requires service mode before enabling', async ({ onTestFinished }) => {
  const view = await setup(onTestFinished)
  backend.values.enable_service_mode = false
  await view.rerender(<TransparentProxyButton />)
  await expect
    .element(
      view.getByRole('button', {
        name: m.settings_clash_transparent_proxy_label(),
        exact: false,
      }),
    )
    .toBeDisabled()
  await expect
    .element(
      view.getByText(
        m.settings_clash_transparent_proxy_service_mode_required(),
      ),
    )
    .toBeInTheDocument()
  expect(backend.upsert).not.toHaveBeenCalled()
})

test('does not enable when the service lacks capture support', async ({
  onTestFinished,
}) => {
  const view = await setup(onTestFinished)
  backend.supported = false
  await view.rerender(<TransparentProxyButton />)
  await expect
    .element(
      view.getByRole('button', {
        name: m.settings_clash_transparent_proxy_label(),
        exact: false,
      }),
    )
    .toBeDisabled()
  await expect
    .element(
      view.getByText(m.settings_clash_transparent_proxy_status_unsupported()),
    )
    .toBeInTheDocument()
})

test('blocks enabling while TUN is active', async ({ onTestFinished }) => {
  const view = await setup(onTestFinished, { enable_tun_mode: true })
  await expect
    .element(
      view.getByRole('button', {
        name: m.settings_clash_transparent_proxy_label(),
        exact: false,
      }),
    )
    .toBeDisabled()
  await expect
    .element(view.getByText(m.error_transparent_proxy_tun_conflict()))
    .toBeInTheDocument()
})

test('allows disabling existing REDIRECT capture when service mode is off', async ({
  onTestFinished,
}) => {
  const view = await setup(onTestFinished, {
    enable_service_mode: false,
    transparent_proxy: {
      mode: 'redir',
      local: true,
      interfaces: [],
      ipv6: false,
    },
  })
  const button = view.getByRole('button', {
    name: m.settings_clash_transparent_proxy_label(),
    exact: false,
  })
  await expect.element(button).toBeEnabled()
  await button.click()
  await expect
    .poll(() => backend.upsert.mock.calls)
    .toEqual([
      [
        'transparent_proxy',
        { mode: 'disabled', local: true, interfaces: [], ipv6: false },
      ],
    ])
})

test('reuses a configured TPROXY port', async ({ onTestFinished }) => {
  const port = { kind: 'fixed', start_port: 9000 }
  const view = await setup(onTestFinished, { tproxy_port: port })
  await view
    .getByRole('button', {
      name: m.settings_clash_transparent_proxy_label(),
      exact: false,
    })
    .click()
  await expect.poll(() => backend.upsert.mock.calls.length).toBe(1)
  expect(backend.upsert.mock.calls[0][0]).toBe('transparent_proxy')
})

test('does not enable capture when port configuration fails', async ({
  onTestFinished,
}) => {
  const view = await setup(onTestFinished)
  backend.upsert.mockRejectedValueOnce(new Error('port unavailable'))
  await view
    .getByRole('button', {
      name: m.settings_clash_transparent_proxy_label(),
      exact: false,
    })
    .click()
  await expect.poll(() => backend.upsert.mock.calls.length).toBe(1)
  expect(backend.upsert.mock.calls[0][0]).toBe('tproxy_port')
})

test('reports pending until the service applies the selected mode', async ({
  onTestFinished,
}) => {
  const view = await setup(onTestFinished, {
    transparent_proxy: {
      mode: 'tproxy',
      local: true,
      interfaces: [],
      ipv6: false,
    },
  })
  await expect
    .element(
      view.getByText(m.settings_clash_transparent_proxy_status_pending()),
    )
    .toBeInTheDocument()
  backend.actualActive = true
  backend.actualMode = 'tproxy'
  await view.rerender(<TransparentProxyButton />)
  await expect
    .element(
      view.getByText(m.settings_clash_transparent_proxy_status_applied()),
    )
    .toBeInTheDocument()
})
