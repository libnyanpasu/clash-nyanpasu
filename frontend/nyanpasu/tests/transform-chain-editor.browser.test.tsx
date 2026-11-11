import { expect, test, vi, type TestContext } from 'vitest'
import { render } from 'vitest-browser-react'
import { page, userEvent } from 'vitest/browser'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { ImportUrlProvider } from '@/components/profile-import/import-url-provider'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import {
  TransformChain,
  TransformChainBar,
  TransformChainHeader,
} from '@/pages/(main)/main/profiles/_modules/transform-chain-list'
import { TransformLibrary } from '@/pages/(main)/main/profiles/_modules/transform-library'
import { useTransformChainDraft } from '@/pages/(main)/main/profiles/_modules/use-transform-chain-draft'
import { type DetailPanelLayout } from '@/pages/(main)/main/profiles/$type/detail/_modules/detail-panel'
import TransformsPanel from '@/pages/(main)/main/profiles/$type/detail/_modules/transforms-panel'
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

vi.hoisted(() => {
  Object.assign(globalThis, { isTauri: true })
})

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

// The global page's chain, composed the way its page does with the library
// below the chain.
function GlobalEditor() {
  const { query, setGlobalTransforms } = useProfile()

  const chain = useTransformChainDraft({
    transforms: query.data?.global_transforms ?? [],
    taskKey: 'test-global',
    onApply: (ids) => setGlobalTransforms.mutateAsync(ids),
  })

  return (
    <TransformChain
      chain={chain}
      traceStepOf={(uid, index) => `global:${uid}:${index}`}
      header={<TransformChainHeader chain={chain} />}
      footer={
        <>
          <TransformLibrary chain={chain} />

          <TransformChainBar chain={chain} />
        </>
      }
    />
  )
}

async function setup(
  layout: Layout,
  active: string[],
  onTestFinished: TestContext['onTestFinished'],
  panelLayout: DetailPanelLayout = 'wide',
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
    const { query } = useProfile()
    const current = query.data?.items.find((item) => item.uid === 'profile')
    if (!current || current.type !== 'config') return null
    return layout === 'scoped' ? (
      <TransformsPanel
        key={current.uid}
        profile={current}
        layout={panelLayout}
        onClose={() => {}}
      />
    ) : (
      <GlobalEditor />
    )
  }
  const container = document.createElement('div')
  document.body.append(container)
  const route = createRootRoute({
    component: () => (
      <TestQueryProvider client={queries}>
        <BlockTaskProvider>
          <TooltipProvider>
            <ImportUrlProvider>
              <div className="bg-surface text-on-surface w-full p-4">
                <Editor />
              </div>
            </ImportUrlProvider>
          </TooltipProvider>
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
  // The library lists every transform; its toggle adds or removes one.
  const toggle = (name: string) =>
    view.getByRole('button', {
      name: m.profile_transform_library_toggle({ name }),
      exact: true,
    })
  const addTransform = (name: string) => toggle(name).click()
  const removeFromRow = (id: string) =>
    userEvent.click(
      item(id).querySelector<HTMLElement>(
        '[data-slot="profile-chain-editor-remove"]',
      )!,
    )
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
    toggle,
    addTransform,
    removeFromRow,
    focusItem,
    queries,
  }
}

test.for(['scoped', 'global'] as const)(
  '%s adds from the library, reorders by dragging, then applies or resets',
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
    // Only the added row is marked, and the bar says what the draft changed.
    expect(
      item('lua').querySelector('[data-slot="profile-chain-editor-unapplied"]'),
    ).not.toBeNull()
    expect(
      item('self').querySelector(
        '[data-slot="profile-chain-editor-unapplied"]',
      ),
    ).toBeNull()
    // The added row grows into place, and a row keeps the transition of its
    // lift although the group sets one for the corners.
    expect(getComputedStyle(item('lua')).animationName).toBe('row-enter')
    expect(getComputedStyle(item('self')).animationName).toBe('none')
    expect(getComputedStyle(item('self')).transitionProperty).toContain('scale')
    await expect
      .element(
        view.getByText(m.profile_chain_editor_summary_added({ count: 1 }), {
          exact: true,
        }),
      )
      .toBeVisible()
    await reset().click()
    await expect.poll(ids).toEqual(['dns', 'self'])
    await expect.element(apply()).not.toBeInTheDocument()

    await addTransform('lua')
    await userEvent.dragAndDrop(item('lua'), item('dns'))
    await expect.poll(ids).toEqual(['lua', 'dns', 'self'])
    await expect
      .element(
        view.getByText(m.profile_chain_editor_summary_added({ count: 1 }), {
          exact: true,
        }),
      )
      .toBeVisible()
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
  '%s shows the empty state, keeps the library stable and removes from the toggle or the row',
  async (layout, { onTestFinished }) => {
    await page.viewport(1100, 1000)
    const { view, writes, ids, toggle, addTransform, removeFromRow, apply } =
      await setup(layout, [], onTestFinished)
    await expect
      .poll(() => view.container.textContent)
      .toContain(m.profile_chain_editor_empty())
    expect(ids()).toEqual([])
    await expect
      .element(toggle('DNS override'))
      .toHaveAttribute('aria-pressed', 'false')

    await addTransform('DNS override')
    await addTransform('lua')
    await expect.poll(ids).toEqual(['dns', 'lua'])
    expect(view.container.textContent).not.toContain(
      m.profile_chain_editor_empty(),
    )

    // Every transform stays listed; chain members are pressed and say where
    // they sit.
    await expect
      .element(toggle('DNS override'))
      .toHaveAttribute('aria-pressed', 'true')
    await expect.element(toggle('lua')).toHaveAttribute('aria-pressed', 'true')
    await expect
      .element(toggle('warp'))
      .toHaveAttribute('aria-pressed', 'false')
    expect(view.container.textContent).toContain(
      m.profile_transform_library_position({ position: 2 }),
    )

    await toggle('DNS override').click()
    await expect.poll(ids).toEqual(['lua'])
    await expect
      .element(toggle('DNS override'))
      .toHaveAttribute('aria-pressed', 'false')
    expect(view.container.textContent).toContain(
      m.profile_transform_library_position({ position: 1 }),
    )

    await removeFromRow('lua')
    await expect.poll(ids).toEqual([])
    await expect.element(toggle('lua')).toHaveAttribute('aria-pressed', 'false')
    await expect.element(apply()).not.toBeInTheDocument()
    expect(writes).toHaveLength(0)
  },
)

test('the bar summarises added, removed and reordered transforms', async ({
  onTestFinished,
}) => {
  await page.viewport(1100, 1000)
  const { view, ids, item, toggle, addTransform } = await setup(
    'scoped',
    ['dns', 'self'],
    onTestFinished,
  )
  await expect.poll(ids).toEqual(['dns', 'self'])

  await addTransform('lua')
  await toggle('DNS override').click()
  await userEvent.dragAndDrop(item('lua'), item('self'))
  await expect.poll(ids).toEqual(['lua', 'self'])
  await expect
    .element(
      view.getByText(
        [
          m.profile_chain_editor_summary_added({ count: 1 }),
          m.profile_chain_editor_summary_removed({ count: 1 }),
        ].join(' · '),
        { exact: true },
      ),
    )
    .toBeVisible()

  // Moving a kept transform is the only change that says the order changed.
  await toggle('lua').click()
  await toggle('DNS override').click()
  await expect.poll(ids).toEqual(['self', 'dns'])
  await expect
    .element(
      view.getByText(m.profile_chain_editor_summary_reordered(), {
        exact: true,
      }),
    )
    .toBeVisible()
})

test('row actions link to the editor and Inspect', async ({
  onTestFinished,
}) => {
  await page.viewport(1100, 1000)
  const { view, writes, ids, item } = await setup(
    'scoped',
    ['dns', 'self', 'warp'],
    onTestFinished,
  )
  await expect.poll(ids).toEqual(['dns', 'self', 'warp'])

  const action = (slot: string) =>
    item('dns').querySelector<HTMLElement>(
      `[data-slot="profile-chain-editor-${slot}"]`,
    )!

  expect(action('trace').getAttribute('href')).toMatch(
    /step=scoped(:|%3A)profile(:|%3A)dns(:|%3A)0$/,
  )
  expect(
    view.container.querySelectorAll(
      '[data-slot="profile-chain-editor-open-editor"]',
    ),
  ).toHaveLength(3)

  await userEvent.click(action('open-editor'))
  await expect
    .poll(() => writes.map((write) => write.method))
    .toEqual(['create_editor_window'])
  expect(writes[0]!.params).toEqual({ windowType: 'profile', uid: 'dns' })
})

test.for(['scoped', 'global'] as const)(
  '%s retains keyboard sorting and cancellation',
  async (layout, { onTestFinished }) => {
    await page.viewport(1100, 1000)
    const { view, writes, ids, item, apply, focusItem } = await setup(
      layout,
      ['dns', 'self', 'warp'],
      onTestFinished,
    )
    await expect.poll(ids).toEqual(['dns', 'self', 'warp'])
    await focusItem('self')
    await userEvent.keyboard('{Space}')
    await expect
      .poll(() => item('self').hasAttribute('data-dragging'))
      .toBe(true)
    for (let i = 0; i < 12; i++) await userEvent.keyboard('{ArrowUp}')
    await expect.poll(() => ids()[0]).toBe('self')
    await userEvent.keyboard('{Escape}')
    await expect.poll(ids).toEqual(['dns', 'self', 'warp'])
    await expect
      .poll(() =>
        view.container.querySelector(
          '[data-slot="profile-chain-editor-item"][data-dragging]',
        ),
      )
      .toBeNull()
    expect(writes).toHaveLength(0)
    await focusItem('self')
    await userEvent.keyboard('{Space}')
    await expect
      .poll(() => item('self').hasAttribute('data-dragging'))
      .toBe(true)
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
  const {
    view,
    ids,
    item,
    queries,
    apply,
    toggle,
    addTransform,
    removeFromRow,
  } = await setup('scoped', ['dns', 'lua'], onTestFinished)
  await expect.poll(ids).toEqual(['dns', 'lua'])
  await addTransform('useself')
  await expect.poll(ids).toEqual(['dns', 'lua', 'self'])
  await removeFromRow('dns')
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
    const data = old as { items: ProfileItem_Serialize[] } | undefined
    // Other queries live in the same cache; only the profile list changes.
    if (!data?.items) return old
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
      page.elementLocator(
        item('dns').querySelector<HTMLElement>(
          '[data-slot="profile-chain-editor-remove"]',
        )!,
      ),
    )
    .toBeDisabled()
  await expect.element(toggle('warp')).toBeDisabled()
})

test('a narrow panel adds several transforms from a drawer that stays open', async ({
  onTestFinished,
}) => {
  await page.viewport(390, 900)
  const { view, ids, apply } = await setup(
    'scoped',
    ['dns'],
    onTestFinished,
    'narrow',
  )
  await expect.poll(ids).toEqual(['dns'])
  // The library is not inline; the button opens it.
  expect(
    view.container.querySelector('[data-slot="profile-transform-library"]'),
  ).toBeNull()

  await view
    .getByRole('button', { name: m.profile_chain_editor_add(), exact: true })
    .click()
  const drawer = page.getByRole('dialog', {
    name: m.profile_chain_editor_add(),
  })
  await expect.element(drawer).toBeVisible()
  const toggleIn = (name: string) =>
    drawer.getByRole('button', {
      name: m.profile_transform_library_toggle({ name }),
      exact: true,
    })

  await toggleIn('lua').click()
  await toggleIn('warp').click()
  await expect.poll(ids).toEqual(['dns', 'lua', 'warp'])
  await expect.element(drawer).toBeVisible()
  await expect.element(toggleIn('lua')).toHaveAttribute('aria-pressed', 'true')

  await toggleIn('lua').click()
  await expect.poll(ids).toEqual(['dns', 'warp'])
  await expect.element(drawer).toBeVisible()

  // Escape closes the drawer and keeps the draft, which the bar offers to apply.
  await userEvent.keyboard('{Escape}')
  await expect.element(drawer).not.toBeInTheDocument()
  await expect.poll(ids).toEqual(['dns', 'warp'])
  await expect.element(apply()).toBeVisible()
  await expect
    .element(
      view.getByText(m.profile_chain_editor_summary_added({ count: 1 }), {
        exact: true,
      }),
    )
    .toBeVisible()

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
  }
})

test('the narrow drawer closes with its button', async ({ onTestFinished }) => {
  await page.viewport(390, 900)
  const { view } = await setup('scoped', [], onTestFinished, 'narrow')

  await view
    .getByRole('button', { name: m.profile_chain_editor_add(), exact: true })
    .click()
  const drawer = page.getByRole('dialog', {
    name: m.profile_chain_editor_add(),
  })
  await expect.element(drawer).toBeVisible()
  await drawer.getByRole('button', { name: m.common_close() }).click()
  await expect.element(drawer).not.toBeInTheDocument()
})
