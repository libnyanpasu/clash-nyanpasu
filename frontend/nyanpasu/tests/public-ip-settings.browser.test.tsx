import { ReactNode, useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page, userEvent } from 'vitest/browser'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import type {
  IpLookupProvider,
  StunServer,
  StunServerTest,
} from '@nyanpasu/rpc/types'
import {
  QueryClient,
  QueryClientProvider,
  useMutation,
} from '@tanstack/react-query'
import IpLookupProviderList from '../src/pages/(main)/main/settings/nyanpasu/public-ip/_modules/ip-lookup-provider-list'
import StunServerList from '../src/pages/(main)/main/settings/nyanpasu/public-ip/_modules/stun-server-list'

const BUILTIN: StunServer = {
  host: 'stun.cloudflare.com',
  port: 3478,
  enabled: true,
  capabilities: {
    udp: { ipv4: true, ipv6: true },
    tcp: { ipv4: false, ipv6: false },
  },
  builtin: true,
}

const backend = vi.hoisted(() => ({
  settings: {} as Record<string, unknown>,
  save: vi.fn(),
  test: vi.fn(),
}))
vi.mock('@nyanpasu/query', () => ({
  useSetting: (key: string) => {
    const [value, setValue] = useState(backend.settings[key])
    return {
      value,
      isPending: false,
      upsert: async (next: unknown) => {
        await backend.save(key, next)
        backend.settings[key] = next
        setValue(next)
      },
    }
  },
  useClashSetting: () => ({ value: false }),
  useIpLookupProviders: () => ({
    data: [
      {
        provider: 'dnspod',
        ipv4_url: 'https://ipv4.ddnspod.com',
        ipv6_url: 'https://ipv6.ddnspod.com',
      },
      {
        provider: 'ipify',
        ipv4_url: 'https://api4.ipify.org',
        ipv6_url: 'https://api6.ipify.org',
      },
    ],
  }),
  useStunServerTest: () =>
    useMutation({
      mutationFn: (input: { host: string; port: number }) =>
        backend.test(input),
    }),
}))
vi.mock('@/utils', () => ({ formatError: String }))
vi.mock('@/utils/notification', () => ({ message: vi.fn() }))
vi.mock('@tauri-apps/api/core', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@tauri-apps/api/core')>()),
  isTauri: () => true,
}))

function Providers({ children }: { children: ReactNode }) {
  const [client] = useState(() => new QueryClient())
  return (
    <QueryClientProvider client={client}>
      <TooltipProvider>{children}</TooltipProvider>
    </QueryClientProvider>
  )
}

function serverTest(answered: boolean): StunServerTest {
  const answer = answered
    ? ({ kind: 'answered', address: '203.0.113.7', latency_ms: 21 } as const)
    : ({ kind: 'no_answer' } as const)
  return {
    udp_ipv4: answer,
    udp_ipv6: { kind: 'no_address' },
    tcp_ipv4: { kind: 'no_answer' },
    tcp_ipv6: { kind: 'no_address' },
    capabilities: {
      udp: { ipv4: answered, ipv6: false },
      tcp: { ipv4: false, ipv6: false },
    },
  }
}

test('a lookup provider is enabled only after its privacy notice is accepted', async ({
  onTestFinished,
}) => {
  backend.settings = { ip_lookup_providers: [] as IpLookupProvider[] }
  backend.save.mockResolvedValue(undefined)
  const view = await render(
    <Providers>
      <IpLookupProviderList />
    </Providers>,
  )
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  const ipify = view.getByRole('switch', { name: 'ipify' })
  await expect.element(ipify).not.toBeChecked()

  await ipify.click()
  await expect
    .element(page.getByText('https://api4.ipify.org'))
    .toBeInTheDocument()
  await page.getByRole('button', { name: m.common_cancel() }).click()
  await expect.element(ipify).not.toBeChecked()
  expect(backend.save).not.toHaveBeenCalled()

  await ipify.click()
  await page
    .getByRole('button', { name: m.settings_public_ip_lookup_notice_accept() })
    .click()
  await expect.element(ipify).toBeChecked()
  expect(backend.save.mock.lastCall).toEqual(['ip_lookup_providers', ['ipify']])

  await ipify.click()
  await expect.element(ipify).not.toBeChecked()
  expect(backend.save.mock.lastCall).toEqual(['ip_lookup_providers', []])
})

test('a built-in server can be turned off but not removed', async ({
  onTestFinished,
}) => {
  backend.settings = { stun_servers: [BUILTIN] }
  backend.save.mockResolvedValue(undefined)
  const view = await render(
    <Providers>
      <StunServerList />
    </Providers>,
  )
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  await view
    .getByRole('button', {
      name: m.settings_public_ip_stun_row_actions({
        server: 'stun.cloudflare.com:3478',
      }),
    })
    .click()
  await expect
    .element(
      page.getByRole('menuitem', { name: m.settings_public_ip_stun_move_up() }),
    )
    .toBeInTheDocument()
  await expect
    .element(
      page.getByRole('menuitem', { name: m.settings_public_ip_stun_remove() }),
    )
    .not.toBeInTheDocument()
  await userEvent.keyboard('{Escape}')

  await view.getByRole('switch', { name: 'stun.cloudflare.com:3478' }).click()
  await expect
    .poll(() => backend.save.mock.lastCall)
    .toEqual(['stun_servers', [{ ...BUILTIN, enabled: false }]])
})

test('adding a server tests it and saves what answered', async ({
  onTestFinished,
}) => {
  backend.settings = { stun_servers: [BUILTIN] }
  backend.save.mockResolvedValue(undefined)
  backend.test.mockResolvedValue(serverTest(true))
  const view = await render(
    <Providers>
      <StunServerList />
    </Providers>,
  )
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  await view
    .getByRole('button', { name: m.settings_public_ip_stun_add() })
    .click()
  await page
    .getByLabelText(m.settings_public_ip_stun_host())
    .fill(' stun.example.com ')
  await page
    .getByRole('button', { name: m.settings_public_ip_stun_add_confirm() })
    .click()

  await expect
    .element(page.getByText('203.0.113.7 · 21 ms'))
    .toBeInTheDocument()
  expect(backend.test).toHaveBeenCalledWith({
    host: 'stun.example.com',
    port: 3478,
  })
  expect(backend.save.mock.lastCall).toEqual([
    'stun_servers',
    [
      BUILTIN,
      {
        host: 'stun.example.com',
        port: 3478,
        enabled: true,
        capabilities: serverTest(true).capabilities,
        builtin: false,
      },
    ],
  ])
})

test('a server that answers over nothing is not added', async ({
  onTestFinished,
}) => {
  backend.settings = { stun_servers: [BUILTIN] }
  backend.test.mockResolvedValue(serverTest(false))
  const view = await render(
    <Providers>
      <StunServerList />
    </Providers>,
  )
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  await view
    .getByRole('button', { name: m.settings_public_ip_stun_add() })
    .click()
  await page
    .getByLabelText(m.settings_public_ip_stun_host())
    .fill('stun.example.com')
  await page
    .getByRole('button', { name: m.settings_public_ip_stun_add_confirm() })
    .click()

  await expect
    .element(page.getByText(m.settings_public_ip_stun_result_none()))
    .toBeInTheDocument()
  expect(backend.save).not.toHaveBeenCalled()
})

test('a test that answers after the dialog closes adds nothing', async ({
  onTestFinished,
}) => {
  backend.settings = { stun_servers: [BUILTIN] }
  backend.save.mockResolvedValue(undefined)
  let answer = (_test: StunServerTest) => {}
  backend.test.mockReturnValue(
    new Promise<StunServerTest>((resolve) => {
      answer = resolve
    }),
  )
  const view = await render(
    <Providers>
      <StunServerList />
    </Providers>,
  )
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  await view
    .getByRole('button', { name: m.settings_public_ip_stun_add() })
    .click()
  await page
    .getByLabelText(m.settings_public_ip_stun_host())
    .fill('stun.example.com')
  await page
    .getByRole('button', { name: m.settings_public_ip_stun_add_confirm() })
    .click()
  await expect.poll(() => backend.test.mock.calls.length).toBe(1)
  await page.getByRole('button', { name: m.common_cancel() }).click()
  answer(serverTest(true))

  await expect
    .element(page.getByLabelText(m.settings_public_ip_stun_host()))
    .not.toBeInTheDocument()
  // Let the late answer's continuation run before checking it did nothing.
  await new Promise((resolve) => setTimeout(resolve))
  expect(backend.save).not.toHaveBeenCalled()
})

test('a server already in the list cannot be added again', async ({
  onTestFinished,
}) => {
  backend.settings = { stun_servers: [BUILTIN] }
  const view = await render(
    <Providers>
      <StunServerList />
    </Providers>,
  )
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  await view
    .getByRole('button', { name: m.settings_public_ip_stun_add() })
    .click()
  await page
    .getByLabelText(m.settings_public_ip_stun_host())
    .fill('STUN.cloudflare.com')

  await expect
    .element(page.getByText(m.settings_public_ip_stun_duplicate()))
    .toBeInTheDocument()
  await expect
    .element(
      page.getByRole('button', {
        name: m.settings_public_ip_stun_add_confirm(),
      }),
    )
    .toBeDisabled()
  expect(backend.test).not.toHaveBeenCalled()
})

test('dragging a server saves the new order', async ({ onTestFinished }) => {
  const google: StunServer = {
    ...BUILTIN,
    host: 'stun.l.google.com',
    port: 19302,
  }
  backend.settings = { stun_servers: [BUILTIN, google] }
  backend.save.mockResolvedValue(undefined)
  const view = await render(
    <Providers>
      <StunServerList />
    </Providers>,
  )
  onTestFinished(async () => {
    await view.unmount()
    vi.clearAllMocks()
  })

  const row = view.container.querySelector(
    '[data-slot="stun-server-item"][data-server="stun.l.google.com:19302"]',
  )
  if (!(row instanceof HTMLElement)) {
    throw new Error('Server row is not available')
  }
  row.focus()
  await userEvent.keyboard('{Space}')
  for (let index = 0; index < 12; index += 1) {
    await userEvent.keyboard('{ArrowUp}')
  }
  await userEvent.keyboard('{Space}')

  await expect
    .poll(() => backend.save.mock.lastCall)
    .toEqual(['stun_servers', [google, BUILTIN]])
})
