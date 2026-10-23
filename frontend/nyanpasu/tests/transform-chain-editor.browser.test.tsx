import { expect, test, vi, type TestContext } from 'vitest'
import { render } from 'vitest-browser-react'
import { page, userEvent } from 'vitest/browser'
import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import { TransformChainList } from '@/pages/(main)/main/profiles/_modules/transform-chain-list'
import TransformChainSection from '@/pages/(main)/main/profiles/$type/detail/_modules/transform-chain-section'
import { m } from '@/paraglide/messages'
import { useProfile } from '@nyanpasu/query'
import type {
  ProfileDefinition_Deserialize,
  ProfileItem_Serialize,
} from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { TestQueryProvider } from './query-provider'

vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({ isMinimized: async () => false }),
}))

type Layout = 'scoped' | 'global'

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
  layout: Layout,
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
    if (method === 'create_editor_window') {
      writes.push({ method, params })
      return null
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
    return layout === 'scoped' ? (
      <TransformChainSection profile={current} />
    ) : (
      <TransformChainList
        taskKey="test-global"
        traceStepOf={(uid, index) => `global:${uid}:${index}`}
        transforms={query.data?.global_transforms ?? []}
        onApply={(ids) => setGlobalTransforms.mutateAsync(ids)}
      />
    )
  }
  const container = document.createElement('div')
  document.body.append(container)
  const route = createRootRoute({
    component: () => (
      <TestQueryProvider client={queries}>
        <BlockTaskProvider>
          <div className="bg-surface text-on-surface w-full p-4">
            <Editor />
          </div>
        </BlockTaskProvider>
      </TestQueryProvider>
    ),
  })
  const router = createRouter({
    routeTree: route,
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })
  const view = await render(<RouterProvider router={router} />, { container })
  onTestFinished(async () => {
    await view.unmount()
    container.remove()
    queries.clear()
    clearMocks()
    vi.unstubAllGlobals()
  })
  await expect
    .poll(() =>
      view.container.querySelector('[data-slot="profile-chain-editor"]'),
    )
    .not.toBeNull()
  const item = (id: string) =>
    view.container.querySelector<HTMLElement>(
      `[data-slot="profile-chain-editor-item"][data-uid="${id}"]`,
    )!
  const ids = () =>
    Array.from(
      view.container.querySelectorAll(
        '[data-slot="profile-chain-editor-item"]',
      ),
    ).map((el) => el.getAttribute('data-uid'))
  const apply = () =>
    view.getByRole('button', { name: m.common_apply(), exact: true })
  const reset = () =>
    view.getByRole('button', { name: m.common_reset(), exact: true })
  const addTransform = async (name: string) => {
    await view
      .getByRole('button', { name: m.profile_chain_editor_add(), exact: true })
      .click()
    await page.getByRole('menuitem', { name, exact: true }).click()
  }
  const rowAction = async (name: string, action: string) => {
    await view
      .getByRole('button', {
        name: m.profile_chain_editor_row_actions({ name }),
      })
      .click()
    await page.getByRole('menuitem', { name: action, exact: true }).click()
  }
  // dnd-kit makes a sortable focusable in a post-commit effect, so a focus call
  // made as soon as the list renders can be a no-op. Retry until it takes.
  const focusItem = (id: string) =>
    expect
      .poll(() => {
        item(id).focus()
        return document.activeElement === item(id)
      })
      .toBe(true)
  return {
    view,
    writes,
    ids,
    item,
    apply,
    reset,
    addTransform,
    rowAction,
    focusItem,
    queries,
  }
}

test.for(['scoped', 'global'] as const)(
  '%s adds from the menu, reorders by dragging, then applies or resets',
  async (layout, { onTestFinished }) => {
    await page.viewport(1100, 1000)
    const { view, writes, ids, item, apply, reset, addTransform } = await setup(
      layout,
      ['dns', 'self'],
      onTestFinished,
    )
    await expect.poll(ids).toEqual(['dns', 'self'])
    await expect.element(apply()).not.toBeInTheDocument()

    await addTransform('lua')
    await expect.poll(ids).toEqual(['dns', 'self', 'lua'])
    expect(writes).toHaveLength(0)
    await reset().click()
    await expect.poll(ids).toEqual(['dns', 'self'])
    await expect.element(apply()).not.toBeInTheDocument()

    await addTransform('lua')
    await userEvent.dragAndDrop(item('lua'), item('dns'))
    await expect.poll(ids).toEqual(['lua', 'dns', 'self'])
    const next = ids()
    await apply().click()
    await expect.poll(() => writes.length).toBe(1)
    if (layout === 'scoped') {
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
    await expect.element(apply()).not.toBeInTheDocument()
    expect(view.container.textContent).toContain(
      m.profile_chain_editor_enabled_count({ count: 3 }),
    )
  },
)

test.for(['scoped', 'global'] as const)(
  '%s shows the empty state, removes through the row menu and lists the rest in the add menu',
  async (layout, { onTestFinished }) => {
    await page.viewport(1100, 1000)
    const { view, writes, ids, addTransform, rowAction } = await setup(
      layout,
      [],
      onTestFinished,
    )
    await expect
      .poll(() => view.container.textContent)
      .toContain(m.profile_chain_editor_empty())
    expect(ids()).toEqual([])

    await addTransform('DNS override')
    await expect.poll(ids).toEqual(['dns'])
    expect(view.container.textContent).not.toContain(
      m.profile_chain_editor_empty(),
    )

    await view
      .getByRole('button', { name: m.profile_chain_editor_add(), exact: true })
      .click()
    await expect
      .element(page.getByRole('menuitem', { name: 'DNS override' }))
      .not.toBeInTheDocument()
    await expect
      .element(page.getByRole('menuitem', { name: 'lua' }))
      .toBeVisible()
    await userEvent.keyboard('{Escape}')

    await rowAction('DNS override', m.profile_chain_editor_remove())
    await expect.poll(ids).toEqual([])
    expect(writes).toHaveLength(0)
  },
)

test('row menu moves a transform and links to the editor and Inspect', async ({
  onTestFinished,
}) => {
  await page.viewport(1100, 1000)
  const { view, writes, ids, rowAction } = await setup(
    'scoped',
    ['dns', 'self', 'warp'],
    onTestFinished,
  )
  await expect.poll(ids).toEqual(['dns', 'self', 'warp'])

  await rowAction('useself', m.profile_chain_editor_move_up())
  await expect.poll(ids).toEqual(['self', 'dns', 'warp'])
  await rowAction('useself', m.profile_chain_editor_move_down())
  await expect.poll(ids).toEqual(['dns', 'self', 'warp'])

  await view
    .getByRole('button', {
      name: m.profile_chain_editor_row_actions({ name: 'DNS override' }),
    })
    .click()
  await expect
    .element(
      page.getByRole('menuitem', {
        name: m.profile_chain_editor_move_up(),
        exact: true,
      }),
    )
    .toHaveAttribute('aria-disabled', 'true')
  await expect
    .element(
      page.getByRole('menuitem', {
        name: m.profile_chain_editor_trace(),
        exact: true,
      }),
    )
    .toHaveAttribute(
      'href',
      expect.stringMatching(/step=scoped(:|%3A)profile(:|%3A)dns(:|%3A)0$/),
    )
  await page
    .getByRole('menuitem', {
      name: m.profile_chain_editor_open_editor(),
      exact: true,
    })
    .click()
  await expect
    .poll(() => writes.map((write) => write.method))
    .toEqual(['create_editor_window'])
  expect(writes[0]!.params).toEqual({ windowType: 'profile', uid: 'dns' })
})

test.for(['scoped', 'global'] as const)(
  '%s retains keyboard sorting and cancellation',
  async (layout, { onTestFinished }) => {
    await page.viewport(1100, 1000)
    const { writes, ids, apply, focusItem } = await setup(
      layout,
      ['dns', 'self', 'warp'],
      onTestFinished,
    )
    await expect.poll(ids).toEqual(['dns', 'self', 'warp'])
    await focusItem('self')
    await userEvent.keyboard('{Space}')
    for (let i = 0; i < 12; i++) await userEvent.keyboard('{ArrowUp}')
    await expect.poll(() => ids()[0]).toBe('self')
    await userEvent.keyboard('{Escape}')
    await expect.poll(ids).toEqual(['dns', 'self', 'warp'])
    expect(writes).toHaveLength(0)
    await focusItem('self')
    await userEvent.keyboard('{Space}')
    for (let i = 0; i < 12; i++) await userEvent.keyboard('{ArrowUp}')
    await userEvent.keyboard('{Space}')
    await expect.poll(ids).toEqual(['self', 'dns', 'warp'])
    await apply().click()
    await expect.poll(() => writes.length).toBe(1)
  },
)

test('duplicates stay read-only and narrow layouts fit', async ({
  onTestFinished,
}) => {
  const { view, ids, queries, apply, addTransform, rowAction } = await setup(
    'scoped',
    ['dns', 'lua'],
    onTestFinished,
  )
  await expect.poll(ids).toEqual(['dns', 'lua'])
  await addTransform('useself')
  await expect.poll(ids).toEqual(['dns', 'lua', 'self'])
  await rowAction('DNS override', m.profile_chain_editor_remove())
  await expect.poll(ids).toEqual(['lua', 'self'])
  for (const width of [390, 320]) {
    await page.viewport(width, 900)
    const editor = view.container.querySelector<HTMLElement>(
      '[data-slot="profile-chain-editor"]',
    )!
    expect(editor.scrollWidth).toBeLessThanOrEqual(editor.clientWidth)
    const bar = view.container.querySelector<HTMLElement>(
      '[data-slot="profile-chain-editor-bar"]',
    )!
    expect(bar.scrollWidth).toBeLessThanOrEqual(bar.clientWidth)
    await expect.element(apply()).toBeVisible()
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
  await expect
    .element(
      view
        .getByRole('button', {
          name: m.profile_chain_editor_row_actions({ name: 'DNS override' }),
        })
        .first(),
    )
    .toBeDisabled()
  await expect
    .element(
      view.getByRole('button', {
        name: m.profile_chain_editor_add(),
        exact: true,
      }),
    )
    .toBeDisabled()
})
