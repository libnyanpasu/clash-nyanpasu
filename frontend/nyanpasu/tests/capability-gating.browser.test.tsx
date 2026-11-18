import { expect, test, vi } from 'vitest'
import '@nyanpasu/theme/styles/fonts.css'
import '@nyanpasu/theme/styles/theme.css'
import '@/assets/styles/tailwind.css'
import { render } from 'vitest-browser-react'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import {
  RequireCapability,
  useCapabilities,
} from '@/components/auth/capabilities'
import { ReadOnlyScope } from '@/components/auth/read-only-scope'
import { RestrictedContent } from '@/components/auth/restricted-content'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import { DelayHistoryTooltip } from '@/components/proxies/delay-history'
import { m } from '@/paraglide/messages'
import ProxyNodeButton from '../src/pages/(main)/main/proxies/group/_modules/proxy-node-button'
import { withPermissions } from './all-permissions'

function Tiers() {
  const capabilities = useCapabilities()

  return (
    <output>
      {(
        [
          'proxy:read',
          'proxy:write',
          'logs:read',
          'settings:write',
          'system:read',
        ] as const
      )
        .map(
          (permission) =>
            `${permission}:${capabilities.hasPermission(permission)}:${capabilities.isReadOnly(permission)}`,
        )
        .join(' ')}
    </output>
  )
}

test('write includes read on its resource and grants nothing on another', async () => {
  const view = await render(<Tiers />, {
    wrapper: withPermissions({ proxy: 'write' }),
  })

  await expect
    .element(view.getByRole('status'))
    .toHaveTextContent(
      'proxy:read:true:false proxy:write:true:false logs:read:false:true settings:write:false:true system:read:false:true',
    )
})

test('a caller holding every permission of the page sees no banner and live controls', async () => {
  const view = await render(
    <ReadOnlyScope permissions={['settings:write']}>
      <button type="button">save</button>
    </ReadOnlyScope>,
    { wrapper: withPermissions({ settings: 'write' }) },
  )

  await expect.element(view.getByRole('button', { name: 'save' })).toBeEnabled()
  expect(view.getByRole('status').query()).toBeNull()
})

test('a caller without the permission sees a read-only banner and inert controls', async () => {
  const view = await render(
    <ReadOnlyScope permissions={['settings:write', 'system:write']}>
      <button type="button">save</button>
      <input aria-label="name" />
    </ReadOnlyScope>,
    // `settings:write` alone does not cover a page that also needs `system:write`.
    { wrapper: withPermissions({ settings: 'write', system: 'read' }) },
  )

  await expect
    .element(view.getByRole('status'))
    .toHaveTextContent(m.auth_read_only_banner())
  await expect
    .element(view.getByRole('button', { name: 'save' }))
    .toBeDisabled()
  await expect.element(view.getByLabelText('name')).toBeDisabled()
})

test('the banner is left to one sibling scope when another asks for none', async () => {
  const view = await render(
    <>
      <ReadOnlyScope permissions={['system:write']}>
        <button type="button">one</button>
      </ReadOnlyScope>
      <ReadOnlyScope permissions={['system:write']} banner={false}>
        <button type="button">two</button>
      </ReadOnlyScope>
    </>,
    {
      wrapper: withPermissions({
        proxy: 'read',
        profile: 'read',
        settings: 'read',
        system: 'read',
        logs: 'read',
        traffic: 'read',
      }),
    },
  )

  await expect.element(view.getByRole('button', { name: 'two' })).toBeDisabled()
  expect(view.getByRole('status').elements()).toHaveLength(1)
})

test('a view of secrets is not mounted without its domain read', async () => {
  const mounted = vi.fn()
  function SecretView() {
    mounted()
    return <p>secret</p>
  }

  const denied = await render(
    <RequireCapability
      permission="profile:read"
      fallback={<RestrictedContent />}
    >
      <SecretView />
    </RequireCapability>,
    { wrapper: withPermissions({ proxy: 'write', logs: 'read' }) },
  )

  await expect
    .element(denied.getByText(m.auth_restricted_title()))
    .toBeVisible()
  expect(mounted).not.toHaveBeenCalled()
  await denied.unmount()

  const allowed = await render(
    <RequireCapability
      permission="profile:read"
      fallback={<RestrictedContent />}
    >
      <SecretView />
    </RequireCapability>,
    { wrapper: withPermissions({ profile: 'read' }) },
  )

  await expect.element(allowed.getByText('secret')).toBeVisible()
})

async function renderNode(held: Parameters<typeof withPermissions>[0]) {
  const onSelect = vi.fn(async () => {})
  const onDelayTest = vi.fn(async () => {})
  const view = await render(
    <BlockTaskProvider>
      <TooltipProvider>
        <DelayHistoryTooltip>
          <ProxyNodeButton
            proxy={{
              name: 'node-a',
              type: 'Vless',
              udp: true,
              history: [],
              id: null,
              now: null,
              all: null,
              testUrl: null,
              expectedStatus: null,
              fixed: null,
              hidden: null,
              icon: null,
              emptyFallback: null,
              provider: null,
            }}
            selectable
            fixed={false}
            onSelect={onSelect}
            onDelayTest={onDelayTest}
            history={[]}
            delay={120}
          />
        </DelayHistoryTooltip>
      </TooltipProvider>
    </BlockTaskProvider>,
    { wrapper: withPermissions(held) },
  )

  return { view, onSelect, onDelayTest }
}

test('selecting a node needs proxy:write', async () => {
  const denied = await renderNode({
    proxy: 'read',
    profile: 'read',
    settings: 'read',
    system: 'read',
    logs: 'read',
    traffic: 'read',
  })
  await denied.view.getByText('node-a').click()
  expect(denied.onSelect).not.toHaveBeenCalled()
  await denied.view.unmount()

  const allowed = await renderNode({ proxy: 'write' })
  await allowed.view.getByText('node-a').click()
  await vi.waitFor(() => expect(allowed.onSelect).toHaveBeenCalledOnce())
  await allowed.view.unmount()
})
