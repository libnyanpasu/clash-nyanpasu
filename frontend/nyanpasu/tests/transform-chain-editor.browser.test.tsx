import { expect, test, vi, type TestContext } from 'vitest'
import { render } from 'vitest-browser-react'
import { page, userEvent } from 'vitest/browser'
import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import ChianEditorCard, {
  TransformChainEditor,
} from '@/pages/(main)/main/profiles/$type/detail/_modules/chian-editor-card'
import type { TransformChainLayout } from '@/pages/(main)/main/profiles/$type/detail/_modules/transform-chain-column'
import { m } from '@/paraglide/messages'
import { useProfile } from '@nyanpasu/query'
import type {
  ProfileDefinition_Deserialize,
  ProfileItem_Serialize,
} from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { TestQueryProvider } from './query-provider'

vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({ isMinimized: async () => false }),
}))

const transforms: ProfileItem_Serialize[] = [
  {
    uid: 'dns',
    name: 'DNS override',
    type: 'transform',
    transform: {
      type: 'overlay',
      source: { type: 'local', binding: { type: 'managed', file: 'dns.yaml' } },
    },
  },
  ...(['self', 'warp', 'lua'] as const).map((uid): ProfileItem_Serialize => ({
    uid,
    name: uid === 'self' ? 'useself' : uid,
    type: 'transform',
    transform: {
      type: 'script',
      runtime: uid === 'lua' ? 'lua' : 'javascript',
      source: {
        type: 'local',
        binding: { type: 'managed', file: `${uid}.js` },
      },
    },
  })),
]

async function setup(
  layout: TransformChainLayout,
  active: string[],
  onTestFinished: TestContext['onTestFinished'],
) {
  vi.stubGlobal('isTauri', true)
  let config = {
    uid: 'profile',
    name: 'Remote Profile',
    type: 'config',
    config: {
      type: 'file',
      source: {
        type: 'local',
        binding: { type: 'managed', file: 'config.yaml' },
      },
      transforms: active,
    },
  } satisfies ProfileItem_Serialize
  let globalTransforms = [...active]
  const writes: { method: string; params: Record<string, unknown> }[] = []
  mockIPC((command, args) => {
    expect(command).toBe('call_rpc')
    const { method, params } = args as {
      method: string
      params: Record<string, unknown>
    }
    if (method === 'get_profiles')
      return {
        current: 'profile',
        items: [config, ...transforms],
        global_transforms: globalTransforms,
        valid: [],
      }
    if (method === 'replace_profile_definition') {
      writes.push({ method, params })
      const definition = params.definition as ProfileDefinition_Deserialize
      if (definition.type !== 'config' || definition.config.type !== 'file')
        throw new Error('Unexpected definition')
      config = {
        ...config,
        config: {
          ...config.config,
          transforms: definition.config.transforms ?? [],
        },
      }
      return { status: 'committed', value: null }
    }
    if (method === 'set_global_transforms') {
      writes.push({ method, params })
      globalTransforms = params.ids as string[]
      return { status: 'committed', value: null }
    }
    throw new Error(`Unexpected RPC: ${method}`)
  })
  const queries = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  function Editor() {
    const { query, setGlobalTransforms } = useProfile()
    const current = query.data?.items.find((item) => item.uid === 'profile')
    if (!current || current.type !== 'config') return null
    return layout === 'stacked' ? (
      <ChianEditorCard profile={current} />
    ) : (
      <TransformChainEditor
        layout="grouped"
        taskKey="test-global"
        transforms={query.data?.global_transforms ?? []}
        onApply={(ids) => setGlobalTransforms.mutateAsync(ids)}
      />
    )
  }
  const view = await render(
    <TestQueryProvider client={queries}>
      <BlockTaskProvider>
        <div className="bg-surface text-on-surface w-full p-4">
          <Editor />
        </div>
      </BlockTaskProvider>
    </TestQueryProvider>,
  )
  onTestFinished(async () => {
    await view.unmount()
    queries.clear()
    clearMocks()
    vi.unstubAllGlobals()
  })
  await expect
    .poll(
      () =>
        view.container.querySelectorAll(
          '[data-slot="profile-chain-editor-item"]',
        ).length,
    )
    .toBe(4)
  const column = (type: string) =>
    view.container.querySelector(`[data-column="${type}"]`)!
  const ids = (type: string) =>
    Array.from(column(type).querySelectorAll('[data-uid]')).map((el) =>
      el.getAttribute('data-uid'),
    )
  const item = (id: string) =>
    view.container.querySelector<HTMLElement>(`[data-uid="${id}"]`)!
  return { view, writes, ids, column, item, queries }
}

test.for(['stacked', 'grouped'] as const)(
  '%s preserves cross-area dragging, explicit apply and reset',
  async (layout, { onTestFinished }) => {
    await page.viewport(1100, 1000)
    const { view, writes, ids, column, item } = await setup(
      layout,
      ['dns', 'self'],
      onTestFinished,
    )
    const target =
      column('active').querySelector<HTMLElement>('[data-uid="dns"]')!
    await userEvent.dragAndDrop(item('lua'), target)
    await expect.poll(() => ids('active')).toContain('lua')
    await expect.poll(() => ids('active')).toHaveLength(3)
    expect(writes).toHaveLength(0)
    await view
      .getByRole('button', { name: m.common_reset(), exact: true })
      .click()
    await expect.poll(() => ids('active')).toEqual(['dns', 'self'])
    await userEvent.dragAndDrop(
      item('lua'),
      column('active').querySelector<HTMLElement>('[data-uid="self"]')!,
    )
    await expect.poll(() => ids('active')).toContain('lua')
    await expect.poll(() => ids('active')).toHaveLength(3)
    const next = ids('active')
    await view
      .getByRole('button', { name: m.common_apply(), exact: true })
      .click()
    await expect.poll(() => writes.length).toBe(1)
    if (layout === 'stacked') {
      expect(writes[0]).toEqual({
        method: 'replace_profile_definition',
        params: {
          uid: 'profile',
          definition: {
            type: 'config',
            config: {
              type: 'file',
              source: {
                type: 'local',
                binding: { type: 'managed', file: 'config.yaml' },
              },
              transforms: next,
            },
          },
        },
      })
    } else
      expect(writes[0]).toEqual({
        method: 'set_global_transforms',
        params: { ids: next },
      })
    await expect
      .poll(() =>
        view
          .getByRole('button', { name: m.common_apply(), exact: true })
          .query(),
      )
      .toBeNull()
  },
)

test.for(['stacked', 'grouped'] as const)(
  '%s accepts drops into an empty area and dragging back removes the transform',
  async (layout, { onTestFinished }) => {
    await page.viewport(1100, 1000)
    const { view, writes, ids, column, item } = await setup(
      layout,
      [],
      onTestFinished,
    )
    await userEvent.dragAndDrop(
      item('dns'),
      column('active').querySelector<HTMLElement>(
        '[data-slot="profile-chain-editor-empty"]',
      )!,
    )
    await expect.poll(() => ids('active')).toEqual(['dns'])
    await userEvent.dragAndDrop(
      item('dns'),
      column('inactive').querySelector<HTMLElement>('[data-uid="self"]')!,
    )
    await expect.poll(() => ids('active')).toEqual([])
    expect(writes).toHaveLength(0)
    expect(view.container.textContent).toContain(
      m.profile_chain_editor_active_empty(),
    )
  },
)

test.for(['stacked', 'grouped'] as const)(
  '%s retains keyboard sorting and cancellation',
  async (layout, { onTestFinished }) => {
    await page.viewport(1100, 1000)
    const { view, writes, ids, item } = await setup(
      layout,
      ['dns', 'self', 'warp'],
      onTestFinished,
    )
    item('self').focus()
    await userEvent.keyboard('{Space}')
    for (let i = 0; i < 12; i++) await userEvent.keyboard('{ArrowUp}')
    await expect.poll(() => ids('active')[0]).toBe('self')
    await userEvent.keyboard('{Escape}')
    await expect.poll(() => ids('active')).toEqual(['dns', 'self', 'warp'])
    expect(writes).toHaveLength(0)
    item('self').focus()
    await userEvent.keyboard('{Space}')
    for (let i = 0; i < 12; i++) await userEvent.keyboard('{ArrowUp}')
    await userEvent.keyboard('{Space}')
    await expect.poll(() => ids('active')).toEqual(['self', 'dns', 'warp'])
    await view
      .getByRole('button', { name: m.common_apply(), exact: true })
      .click()
    await expect.poll(() => writes.length).toBe(1)
  },
)

test('quick actions edit a draft, duplicates stay read-only, and narrow layouts fit', async ({
  onTestFinished,
}) => {
  const { view, writes, ids, item, queries } = await setup(
    'stacked',
    ['dns'],
    onTestFinished,
  )
  await view
    .getByRole('button', {
      name: m.profile_chain_editor_add_item({ name: 'lua' }),
    })
    .click()
  await expect.poll(() => ids('active')).toEqual(['dns', 'lua'])
  await view
    .getByRole('button', {
      name: m.profile_chain_editor_remove_item({ name: 'DNS override' }),
    })
    .click()
  await expect.poll(() => ids('active')).toEqual(['lua'])
  expect(writes).toHaveLength(0)
  for (const width of [390, 320]) {
    await page.viewport(width, 900)
    const editor = view.container.querySelector<HTMLElement>(
      '[data-slot="profile-chain-editor"]',
    )!
    expect(editor.scrollWidth).toBeLessThanOrEqual(editor.clientWidth)
  }
  queries.setQueriesData({ predicate: () => true }, (old: unknown) => {
    const data = old as { items: ProfileItem_Serialize[] }
    return {
      ...data,
      items: data.items.map((item) =>
        item.type === 'config'
          ? { ...item, config: { ...item.config, transforms: ['dns', 'dns'] } }
          : item,
      ),
    }
  })
  await expect.element(view.getByRole('alert')).toBeVisible()
  expect(item('dns').hasAttribute('role')).toBe(false)
  await expect
    .element(
      view
        .getByRole('button', {
          name: m.profile_chain_editor_remove_item({ name: 'DNS override' }),
        })
        .first(),
    )
    .toBeDisabled()
})
