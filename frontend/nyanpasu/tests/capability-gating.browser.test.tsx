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
import { withScopes } from './all-scopes'

function Tiers() {
  const capabilities = useCapabilities()

  return (
    <output>
      {(['read', 'sensitive_read', 'proxy', 'settings', 'system'] as const)
        .map(
          (scope) =>
            `${scope}:${capabilities.hasScope(scope)}:${capabilities.isReadOnly(scope)}`,
        )
        .join(' ')}
    </output>
  )
}

test('each tier is independent: holding proxy grants no read', async () => {
  const view = await render(<Tiers />, { wrapper: withScopes(['proxy']) })

  await expect
    .element(view.getByRole('status'))
    .toHaveTextContent(
      'read:false:true sensitive_read:false:true proxy:true:false settings:false:true system:false:true',
    )
})

test('a caller holding every scope of the page sees no banner and live controls', async () => {
  const view = await render(
    <ReadOnlyScope scopes={['settings']}>
      <button type="button">save</button>
    </ReadOnlyScope>,
    { wrapper: withScopes(['read', 'settings']) },
  )

  await expect.element(view.getByRole('button', { name: 'save' })).toBeEnabled()
  expect(view.getByRole('status').query()).toBeNull()
})

test('a caller without the scope sees a read-only banner and inert controls', async () => {
  const view = await render(
    <ReadOnlyScope scopes={['settings', 'system']}>
      <button type="button">save</button>
      <input aria-label="name" />
    </ReadOnlyScope>,
    // `settings` alone does not cover a page that also needs `system`.
    { wrapper: withScopes(['read', 'settings']) },
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
      <ReadOnlyScope scopes={['system']}>
        <button type="button">one</button>
      </ReadOnlyScope>
      <ReadOnlyScope scopes={['system']} banner={false}>
        <button type="button">two</button>
      </ReadOnlyScope>
    </>,
    { wrapper: withScopes(['read']) },
  )

  await expect.element(view.getByRole('button', { name: 'two' })).toBeDisabled()
  expect(view.getByRole('status').elements()).toHaveLength(1)
})

test('a view of secrets is not mounted without sensitive_read', async () => {
  const mounted = vi.fn()
  function SecretView() {
    mounted()
    return <p>secret</p>
  }

  const denied = await render(
    <RequireCapability scope="sensitive_read" fallback={<RestrictedContent />}>
      <SecretView />
    </RequireCapability>,
    { wrapper: withScopes(['read', 'proxy']) },
  )

  await expect
    .element(denied.getByText(m.auth_restricted_title()))
    .toBeVisible()
  expect(mounted).not.toHaveBeenCalled()
  await denied.unmount()

  const allowed = await render(
    <RequireCapability scope="sensitive_read" fallback={<RestrictedContent />}>
      <SecretView />
    </RequireCapability>,
    { wrapper: withScopes(['read', 'sensitive_read']) },
  )

  await expect.element(allowed.getByText('secret')).toBeVisible()
})

async function renderNode(scopes: Parameters<typeof withScopes>[0]) {
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
    { wrapper: withScopes(scopes) },
  )

  return { view, onSelect, onDelayTest }
}

test('selecting a node needs the proxy scope', async () => {
  const denied = await renderNode(['read'])
  await denied.view.getByText('node-a').click()
  expect(denied.onSelect).not.toHaveBeenCalled()
  await denied.view.unmount()

  const allowed = await renderNode(['read', 'proxy'])
  await allowed.view.getByText('node-a').click()
  await vi.waitFor(() => expect(allowed.onSelect).toHaveBeenCalledOnce())
  await allowed.view.unmount()
})
