import '@/assets/styles/index.css'
import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import type { TransparentProxyDiagnostics } from '@nyanpasu/rpc/types'
import TransparentProxyDiagnosticsView from '../src/pages/(main)/main/settings/system/proxy/_modules/transparent-proxy-diagnostics'

const backend = vi.hoisted(() => ({
  diagnostics: null as TransparentProxyDiagnostics | null,
  connections: [],
}))

vi.mock('@nyanpasu/query', () => ({
  useTransparentProxyDiagnostics: () => ({
    data: backend.diagnostics,
    isLoading: false,
    isError: false,
  }),
  useClashSystemSetting: (key: string) => ({
    value:
      key === 'transparent_proxy'
        ? { mode: 'tproxy', local: true, interfaces: ['eth0'], ipv6: false }
        : key === 'tproxy_port'
          ? { kind: 'fixed', start_port: 7894 }
          : null,
  }),
  useClashConnectionDetails: () => ({
    data: { sequence: 1, connections: backend.connections },
  }),
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

test('old services report that an update is required', async ({
  onTestFinished,
}) => {
  backend.diagnostics = { kind: 'update_required' }
  const view = await render(<TransparentProxyDiagnosticsView />)
  onTestFinished(() => view.unmount())

  await expect
    .element(
      view.getByText(
        m.settings_system_proxy_transparent_diagnostics_update_required(),
      ),
    )
    .toBeInTheDocument()
})

test('available diagnostics show neutral empty connections and kernel states', async ({
  onTestFinished,
}) => {
  backend.connections = []
  backend.diagnostics = {
    kind: 'available',
    diagnostics: {
      supported: true,
      applied: {
        mode: 'tproxy',
        port: 7894,
        local: true,
        interfaces: ['eth0'],
        ipv6: false,
        protocols: ['tcp', 'udp'],
      },
      stale: false,
      marks: { capture_mark: 65536, capture_mask: 65536, egress_mark: 131072 },
      routing: { rule_priority: 10000, table: 233 },
      bypass: {
        root_uid: true,
        local_destination: true,
        private_reserved: true,
        core_egress_mark: true,
      },
      objects: [
        { object: 'nft_table', expected: true, state: 'present' },
        { object: 'nft_output_chain', expected: true, state: 'absent' },
      ],
    },
  }
  const view = await render(<TransparentProxyDiagnosticsView />)
  onTestFinished(() => view.unmount())

  await expect
    .element(
      view.getByText(
        m.settings_system_proxy_transparent_diagnostics_connections_empty(),
      ),
    )
    .toBeInTheDocument()
  await expect
    .element(
      view.getByText(
        m.settings_system_proxy_transparent_diagnostics_object_nft_table(),
      ),
    )
    .toBeInTheDocument()
  await expect
    .element(
      view.getByText(
        m.settings_system_proxy_transparent_diagnostics_state_present(),
      ),
    )
    .toBeInTheDocument()
})
