import '@/assets/styles/index.css'
import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import {
  SystemProxyButton,
  TunModeButton,
  TransparentProxyButton as WidgetTransparentProxyButton,
} from '@/components/settings/system-proxy'
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

vi.mock('@nyanpasu/platform', () => ({ isLinux: true, isTauri: () => true }))
vi.mock('@nyanpasu/query', () => ({
  useSetting: (key: string) => ({ value: backend.values[key] }),
  useClashSetting: (key: string) => ({ value: backend.values[key] }),
  useClashSettings: () => {
    const [value, setValue] = useState(backend.values)
    return {
      query: { data: value },
      upsert: {
        mutateAsync: async (patch: Record<string, unknown>) => {
          await backend.upsert(patch)
          setValue((previous) => ({
            ...previous,
            ...patch,
            transparent_proxy: {
              ...(previous.transparent_proxy as object),
              ...(patch.transparent_proxy as object),
            },
          }))
        },
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
  Link: ({
    children,
    to,
    ...props
  }: {
    children: React.ReactNode
    to: string
  }) => (
    <a href={to} {...props}>
      {children}
    </a>
  ),
}))
vi.mock('@/components/providers/block-task-provider', () => ({
  useBlockTask: (_key: string, execute: () => Promise<void>) => ({
    execute,
    isPending: false,
  }),
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
  await expect.poll(() => backend.upsert.mock.calls.length).toBe(1)
  expect(backend.upsert.mock.calls[0][0]).toEqual({
    tproxy_port: { kind: 'fixed', start_port: 7894 },
    transparent_proxy: { mode: 'tproxy' },
  })
  await expect.element(button).toHaveAttribute('data-active', 'true')
  await button.click()
  await expect.poll(() => backend.upsert.mock.calls.length).toBe(2)
  expect(backend.upsert.mock.calls[1][0]).toEqual({
    transparent_proxy: { mode: 'disabled' },
  })
})

test('opens a dialog with a link to service settings when service mode is off', async ({
  onTestFinished,
}) => {
  const view = await setup(onTestFinished, { enable_service_mode: false })
  const button = view.getByRole('button', {
    name: m.settings_clash_transparent_proxy_label(),
    exact: false,
  })
  await expect
    .element(
      view.getByText(
        m.settings_clash_transparent_proxy_service_mode_required(),
      ),
    )
    .not.toBeInTheDocument()
  await button.click()
  await expect
    .element(
      view.getByText(
        m.settings_clash_transparent_proxy_service_mode_required(),
      ),
    )
    .toBeInTheDocument()
  await expect
    .element(view.getByRole('link', { name: m.settings_label_service() }))
    .toHaveAttribute('href', '/main/settings/system/service')
  const link = view
    .getByRole('link', { name: m.settings_label_service() })
    .element()
  const text = link.querySelector('[data-slot="button-content"]')!
  const range = document.createRange()
  range.selectNodeContents(text)
  const label = range.getBoundingClientRect()
  const bounds = link.getBoundingClientRect()
  expect(
    Math.abs((label.top + label.bottom - bounds.top - bounds.bottom) / 2),
  ).toBeLessThan(1)
  expect(backend.upsert).not.toHaveBeenCalled()
})

test('shows the unsupported-service reason on click', async ({
  onTestFinished,
}) => {
  const view = await setup(onTestFinished)
  backend.supported = false
  await view.rerender(<TransparentProxyButton />)
  await view
    .getByRole('button', {
      name: m.settings_clash_transparent_proxy_label(),
      exact: false,
    })
    .click()
  await expect
    .element(
      view.getByText(m.settings_clash_transparent_proxy_status_unsupported()),
    )
    .toBeInTheDocument()
  await expect
    .element(view.getByRole('link', { name: m.settings_label_network() }))
    .toHaveAttribute('href', '/main/settings/clash/network')
  expect(backend.upsert).not.toHaveBeenCalled()
})

test('shows the TUN conflict on click', async ({ onTestFinished }) => {
  const view = await setup(onTestFinished, { enable_tun_mode: true })
  await view
    .getByRole('button', {
      name: m.settings_clash_transparent_proxy_label(),
      exact: false,
    })
    .click()
  await expect
    .element(view.getByText(m.error_transparent_proxy_tun_conflict()))
    .toBeInTheDocument()
  expect(backend.upsert).not.toHaveBeenCalled()
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
    .toEqual([[{ transparent_proxy: { mode: 'disabled' } }]])
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
  expect(backend.upsert.mock.calls[0][0]).toEqual({
    transparent_proxy: { mode: 'tproxy' },
  })
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
  expect(backend.upsert.mock.calls[0][0]).toEqual({
    transparent_proxy: { mode: 'tproxy' },
    tproxy_port: { kind: 'fixed', start_port: 7894 },
  })
})

test('keeps the same button shape and surface when activation is unavailable', async ({
  onTestFinished,
}) => {
  backend.values = {
    core: 'mihomo',
    enable_service_mode: false,
    enable_system_proxy: false,
    enable_tun_mode: false,
    tproxy_port: null,
    transparent_proxy: { mode: 'disabled' },
  }
  const view = await render(
    <>
      <SystemProxyButton />
      <TransparentProxyButton />
    </>,
  )
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })
  const system = view
    .getByRole('button', { name: m.settings_system_proxy_system_proxy_label() })
    .element()
  const transparent = view
    .getByRole('button', {
      name: m.settings_clash_transparent_proxy_label(),
      exact: false,
    })
    .element()
  const first = getComputedStyle(system)
  const second = getComputedStyle(transparent)
  expect(second.borderRadius).toBe(first.borderRadius)
  expect(second.backgroundColor).toBe(first.backgroundColor)
  expect(second.height).toBe(first.height)
  expect(second.boxShadow).toBe(first.boxShadow)
})

test('TUN cannot be enabled while transparent capture is configured', async ({
  onTestFinished,
}) => {
  backend.values = {
    enable_tun_mode: false,
    transparent_proxy: { mode: 'tproxy' },
  }
  const view = await render(<TunModeButton />)
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })
  await expect
    .element(
      view.getByRole('button', {
        name: m.settings_system_proxy_tun_mode_label(),
      }),
    )
    .toBeDisabled()
})

test('the dashboard shortcut uses the same capture action', async ({
  onTestFinished,
}) => {
  backend.values = {
    core: 'mihomo',
    enable_service_mode: true,
    enable_tun_mode: false,
    tproxy_port: null,
    transparent_proxy: {
      mode: 'disabled',
      local: true,
      interfaces: [],
      ipv6: false,
    },
  }
  backend.supported = true
  backend.upsert.mockResolvedValue(undefined)
  backend.invalidate.mockResolvedValue(undefined)
  const view = await render(<WidgetTransparentProxyButton />)
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })
  const button = view.getByRole('switch', {
    name: m.settings_clash_transparent_proxy_label(),
  })
  await expect.element(button).toHaveAttribute('aria-checked', 'false')
  await button.click()
  await expect
    .poll(() => backend.upsert.mock.calls[0]?.[0])
    .toEqual({
      transparent_proxy: { mode: 'tproxy' },
      tproxy_port: { kind: 'fixed', start_port: 7894 },
    })
  await expect.element(button).toHaveAttribute('aria-checked', 'true')
})
