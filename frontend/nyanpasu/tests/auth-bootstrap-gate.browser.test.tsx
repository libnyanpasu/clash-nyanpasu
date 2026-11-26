import { useEffect } from 'react'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import {
  AuthBootstrapGate,
  type AuthBootstrapGateProps,
} from '@/components/auth/auth-bootstrap-gate'
import {
  RequireCapability,
  useCapabilities,
  type RpcDescribeSnapshot,
} from '@/components/auth/capabilities'
import {
  PRINCIPAL_MARKER_KEY,
  principalKey,
} from '@/components/auth/principal-marker'
import {
  THEME_CSS_VARS_KEY,
  THEME_PALETTE_KEY,
} from '@/components/providers/theme-provider'
import { RpcFailure } from '@nyanpasu/rpc'
import { QueryClient } from '@tanstack/react-query'

function snapshot(
  overrides: Partial<RpcDescribeSnapshot> = {},
): RpcDescribeSnapshot {
  return {
    protocol: 2,
    server: { kind: 'desktop', version: '2.0.0' },
    transport: 'http',
    principal: { kind: 'token', id: 'a1b2', name: 'laptop' },
    role: null,
    permissions: { proxy: 'write', profile: 'read' },
    commands: [
      {
        name: 'get_profiles',
        effect: 'query',
        permission: { resource: 'profile', access: 'read' },
        sensitive: false,
        allowed: true,
      },
      {
        name: 'select_proxy',
        effect: 'mutation',
        permission: { resource: 'proxy', access: 'write' },
        sensitive: false,
        allowed: true,
      },
      {
        name: 'restart_core',
        effect: 'mutation',
        permission: { resource: 'system', access: 'write' },
        sensitive: false,
        allowed: false,
      },
      {
        name: 'get_secret',
        effect: 'query',
        permission: { resource: 'profile', access: 'read' },
        sensitive: true,
        allowed: true,
      },
    ],
    ...overrides,
  }
}

const unauthorized = () =>
  new RpcFailure('rpc_describe', {
    kind: 'unauthorized',
    message: 'Authentication required',
  })

const kvKey = `nyanpasu-kv-:${btoa('custom-css')}`

function seedPersistedState() {
  localStorage.setItem(kvKey, '"x"')
  localStorage.setItem(THEME_PALETTE_KEY, '{}')
  localStorage.setItem(THEME_CSS_VARS_KEY, '""')
  localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn')
  const style = document.createElement('style')
  style.id = 'nyanpasu-custom-css-item-a'
  document.head.appendChild(style)
}

beforeEach(() => localStorage.clear())
afterEach(() => {
  localStorage.clear()
  document.getElementById('nyanpasu-custom-css-item-a')?.remove()
})

async function renderGate(
  props: Partial<AuthBootstrapGateProps> & {
    describe: AuthBootstrapGateProps['describe']
  },
  children: React.ReactNode = <p>app</p>,
) {
  const queryClient = props.queryClient ?? new QueryClient()
  return {
    queryClient,
    screen: await render(
      <AuthBootstrapGate queryClient={queryClient} {...props}>
        {children}
      </AuthBootstrapGate>,
    ),
  }
}

test('401 goes to the login page without mounting children', async () => {
  const navigateToLogin = vi.fn()
  const mounted = vi.fn()
  function Child() {
    useEffect(mounted, [])
    return <p>app</p>
  }

  const { screen } = await renderGate(
    { describe: () => Promise.reject(unauthorized()), navigateToLogin },
    <Child />,
  )

  await vi.waitFor(() => expect(navigateToLogin).toHaveBeenCalledOnce())
  expect(mounted).not.toHaveBeenCalled()
  await expect.element(screen.getByText('app')).not.toBeInTheDocument()
})

test('a changed principal is cleared synchronously before children render', async () => {
  seedPersistedState()
  localStorage.setItem(PRINCIPAL_MARKER_KEY, 'someone-else')
  const queryClient = new QueryClient()
  queryClient.setQueryData(['profiles'], [1])
  const seenByChild: Record<string, unknown> = {}
  function Child() {
    // Rendering is the first moment providers could read stale state.
    seenByChild.kv = localStorage.getItem(kvKey)
    seenByChild.palette = localStorage.getItem(THEME_PALETTE_KEY)
    seenByChild.cssVars = localStorage.getItem(THEME_CSS_VARS_KEY)
    seenByChild.language = localStorage.getItem(
      btoa('paraglide-language-cache'),
    )
    seenByChild.style = document.getElementById('nyanpasu-custom-css-item-a')
    seenByChild.queries = queryClient.getQueryCache().getAll().length
    return <p>app</p>
  }
  const current = snapshot()

  const { screen } = await renderGate(
    { describe: async () => current, queryClient },
    <Child />,
  )

  await expect.element(screen.getByText('app')).toBeInTheDocument()
  expect(seenByChild).toEqual({
    kv: null,
    palette: null,
    cssVars: null,
    language: null,
    style: null,
    queries: 0,
  })
  expect(localStorage.getItem(PRINCIPAL_MARKER_KEY)).toBe(principalKey(current))
})

test('the same principal keeps its persisted state', async () => {
  seedPersistedState()
  const current = snapshot()
  localStorage.setItem(PRINCIPAL_MARKER_KEY, principalKey(current))

  const { screen } = await renderGate({ describe: async () => current })

  await expect.element(screen.getByText('app')).toBeInTheDocument()
  expect(localStorage.getItem(kvKey)).toBe('"x"')
  expect(localStorage.getItem(THEME_PALETTE_KEY)).toBe('{}')
  expect(document.getElementById('nyanpasu-custom-css-item-a')).not.toBeNull()
})

test('a changed permission set is a different principal', async () => {
  seedPersistedState()
  localStorage.setItem(PRINCIPAL_MARKER_KEY, principalKey(snapshot()))

  const { screen } = await renderGate({
    describe: async () => snapshot({ permissions: { profile: 'read' } }),
  })

  await expect.element(screen.getByText('app')).toBeInTheDocument()
  expect(localStorage.getItem(kvKey)).toBeNull()
})

test('another tab binding a different principal reloads this page', async () => {
  const reload = vi.fn()
  const current = snapshot()
  const { screen } = await renderGate({ describe: async () => current, reload })
  await expect.element(screen.getByText('app')).toBeInTheDocument()

  const marker = principalKey(current)
  window.dispatchEvent(
    new StorageEvent('storage', {
      key: PRINCIPAL_MARKER_KEY,
      newValue: marker,
    }),
  )
  window.dispatchEvent(
    new StorageEvent('storage', { key: 'other', newValue: 'x' }),
  )
  expect(reload).not.toHaveBeenCalled()

  window.dispatchEvent(
    new StorageEvent('storage', { key: PRINCIPAL_MARKER_KEY, newValue: 'x' }),
  )
  expect(reload).toHaveBeenCalledOnce()
  window.dispatchEvent(
    new StorageEvent('storage', { key: PRINCIPAL_MARKER_KEY, newValue: null }),
  )
  expect(reload).toHaveBeenCalledTimes(2)
})

test('a describe failure other than 401 renders the error and can retry', async () => {
  const describe = vi
    .fn<AuthBootstrapGateProps['describe']>()
    .mockRejectedValueOnce(new Error('offline'))
    .mockResolvedValueOnce(snapshot())
  const navigateToLogin = vi.fn()

  const { screen } = await renderGate({
    describe,
    navigateToLogin,
    renderError: (error, retry) => (
      <button onClick={retry}>{(error as Error).message}</button>
    ),
  })

  await screen.getByRole('button', { name: 'offline' }).click()

  await expect.element(screen.getByText('app')).toBeInTheDocument()
  expect(navigateToLogin).not.toHaveBeenCalled()
})

function Probe({
  check,
}: {
  check: (c: ReturnType<typeof useCapabilities>) => string
}) {
  return <p>{check(useCapabilities())}</p>
}

async function renderProbe(
  current: RpcDescribeSnapshot,
  check: (c: ReturnType<typeof useCapabilities>) => string,
) {
  const { screen } = await renderGate(
    { describe: async () => current },
    <Probe check={check} />,
  )
  return screen
}

test('useCapabilities enforces permissions and per-command allowance', async () => {
  const screen = await renderProbe(snapshot(), (c) =>
    [
      c.hasPermission('proxy:write'),
      c.hasPermission('system:write'),
      c.hasPermissions(['profile:read', 'proxy:read']),
      c.hasPermissions(['profile:read', 'system:read']),
      c.canCall('select_proxy'),
      c.canCall('restart_core'),
      c.canCall('get_secret'),
      c.canCall('unknown'),
      c.isReadOnly('proxy:write'),
      c.isReadOnly('profile:write'),
      c.isDesktop,
      c.transport,
    ].join(','),
  )

  await expect
    .element(
      screen.getByText(
        'true,false,true,false,true,false,true,false,false,true,false,http',
      ),
    )
    .toBeInTheDocument()
})

test('remote permissions are available only to the desktop principal', async () => {
  const permissions = { profile: 'read', remote: 'write' } as const
  const remote = await renderProbe(
    snapshot({ permissions }),
    (c) =>
      `remote:${c.hasPermission('remote:write')}:${c.isReadOnly('remote:write')}`,
  )
  await expect
    .element(remote.getByText('remote:false:true'))
    .toBeInTheDocument()
  await remote.unmount()

  const desktop = await renderProbe(
    snapshot({ permissions, principal: { kind: 'desktop', window: 'main' } }),
    (c) =>
      `desktop:${c.isDesktop}:${c.hasPermission('remote:write')}:${c.isReadOnly('remote:write')}`,
  )
  await expect
    .element(desktop.getByText('desktop:true:true:false'))
    .toBeInTheDocument()
})

test('RequireCapability renders children or the fallback', async () => {
  const { screen } = await renderGate(
    { describe: async () => snapshot() },
    <>
      <RequireCapability permission="proxy:write" fallback={<p>no proxy</p>}>
        <p>proxy ui</p>
      </RequireCapability>
      <RequireCapability permission="system:write" fallback={<p>no system</p>}>
        <p>system ui</p>
      </RequireCapability>
      <RequireCapability command="get_profiles">
        <p>profiles ui</p>
      </RequireCapability>
      <RequireCapability command="restart_core" fallback={<p>no restart</p>}>
        <p>restart ui</p>
      </RequireCapability>
    </>,
  )

  await expect.element(screen.getByText('proxy ui')).toBeInTheDocument()
  await expect.element(screen.getByText('no system')).toBeInTheDocument()
  await expect.element(screen.getByText('profiles ui')).toBeInTheDocument()
  await expect.element(screen.getByText('no restart')).toBeInTheDocument()
  await expect.element(screen.getByText('system ui')).not.toBeInTheDocument()
})
