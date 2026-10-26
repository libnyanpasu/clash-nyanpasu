import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { page, userEvent } from 'vitest/browser'
import { SnackbarProvider } from '@nyanpasu/ui/snackbar'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import ContextMenuProvider from '@/components/providers/context-menu-provider'
import ProxiesDndProvider from '@/pages/(main)/main/proxies/_modules/proxies-dnd-provider'
import ProxiesLayoutProvider from '@/pages/(main)/main/proxies/_modules/proxies-layout-provider'
import ProxiesNavigate from '@/pages/(main)/main/proxies/_modules/proxies-navigate'
import { m } from '@/paraglide/messages'
import { ask, message } from '@/utils/notification'
import type {
  Proxies_Serialize,
  Proxy_Serialize,
  ProxyGroup,
  ProxyGroupLayout,
} from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router'
import { mockIPC } from '@tauri-apps/api/mocks'
import { drag } from './pointer-drag'
import { TestQueryProvider } from './query-provider'

vi.hoisted(() => {
  vi.stubGlobal('isTauri', true)
})
vi.mock('@/utils/notification', () => ({ message: vi.fn(), ask: vi.fn() }))

const committed = (value: unknown = null) => ({
  status: 'committed',
  value,
  commits: [],
})

const node = (name: string, delay?: number): Proxy_Serialize => ({
  name,
  type: 'Shadowsocks',
  history: delay === undefined ? [] : [{ time: '2026-10-24T00:00:00Z', delay }],
  udp: true,
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
})

const group = (
  name: string,
  type: string,
  now: string | null,
  fixed: string | null = null,
): ProxyGroup => ({
  name,
  type,
  all: now ? [now] : [],
  now,
  fixed,
  testUrl: null,
  expectedStatus: null,
  hidden: false,
  icon: null,
  capabilities: { select: true, clearFixed: true },
})

const PICTURE = 'data:image/gif;base64,R0lGODlhAQABAAAAACw='

// The core reports these four, in this order.
const groups = [
  group('Alpha', 'Selector', 'hk-1', 'hk-1'),
  { ...group('Bravo', 'URLTest', 'jp-1'), icon: PICTURE },
  group('Charlie', 'Fallback', null),
  group('Delta', 'LoadBalance', 'us-1'),
]

const proxies: Proxies_Serialize = {
  global: null,
  groups,
  nodes: {
    'hk-1': node('hk-1', 120),
    'jp-1': node('jp-1', 0),
    'us-1': node('us-1'),
  },
}

const emptyLayout: ProxyGroupLayout = { folders: [], ungrouped: [] }

// Charlie, Alpha and Bravo are loose and Delta is in "Work". Bravo is not in
// the stored layout, so the sidebar appends it.
const arranged: ProxyGroupLayout = {
  folders: [{ id: 'f1', name: 'Work', members: ['Delta'] }],
  ungrouped: ['Charlie', 'Alpha'],
}

const renderAt = async (
  path: string,
  options: {
    stored?: ProxyGroupLayout
    collapsed?: Record<string, true>
    failLayout?: boolean
  } = {},
) => {
  let stored = options.stored
  const layouts: ProxyGroupLayout[] = []
  const created: unknown[] = []
  const writes: unknown[] = []

  mockIPC(
    (command, args) => {
      const wire = args as { method: string; params: Record<string, unknown> }
      const method = command === 'call_rpc' ? wire.method : command

      if (method === 'get_profiles') {
        return {
          current: 'p',
          valid: [],
          global_transforms: [],
          items: [],
          folders: [],
          proxy_group_layouts: stored ? { p: stored } : {},
        }
      }
      if (method === 'get_proxies') return proxies
      if (method === 'get_app_config') {
        return { default_latency_test: 'https://www.gstatic.com/generate_204' }
      }
      if (method === 'get_storage_item') {
        return {
          value: options.collapsed ? JSON.stringify(options.collapsed) : null,
          revision: writes.length,
        }
      }
      if (method === 'set_storage_item') {
        writes.push(JSON.parse(String(wire.params.value)))

        return writes.length
      }
      if (method === 'set_proxy_group_layout') {
        if (options.failLayout) throw new Error('disk full')
        const layout = wire.params.layout as ProxyGroupLayout
        layouts.push(layout)
        stored =
          layout.folders.length + layout.ungrouped.length ? layout : undefined

        return committed()
      }
      if (method === 'create_proxy_group_folder') {
        created.push([wire.params.name, wire.params.members])

        return committed('f9')
      }
      throw new Error(`Unexpected RPC ${method}`)
    },
    { shouldMockEvents: true },
  )

  const queries = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })

  const rootRoute = createRootRoute({
    component: () => (
      <TestQueryProvider client={queries}>
        <BlockTaskProvider>
          <TooltipProvider>
            <SnackbarProvider>
              <ContextMenuProvider>
                <div className="w-80">
                  <ProxiesLayoutProvider>
                    <ProxiesDndProvider>
                      <ProxiesNavigate />
                    </ProxiesDndProvider>
                  </ProxiesLayoutProvider>
                </div>
              </ContextMenuProvider>
            </SnackbarProvider>
          </TooltipProvider>
        </BlockTaskProvider>
      </TestQueryProvider>
    ),
  })
  const router = createRouter({
    routeTree: rootRoute.addChildren([
      createRoute({
        getParentRoute: () => rootRoute,
        path: '/main/proxies/group/$name',
      }),
    ]),
    history: createMemoryHistory({ initialEntries: [path] }),
  })

  const view = await render(<RouterProvider router={router} />)

  await expect.element(groupLink('Alpha')).toBeInTheDocument()

  return { view, layouts, created, writes }
}

const groupLink = (name: string) =>
  page.getByRole('link', { name, exact: false })

const folderHeading = (name: string) =>
  page.getByRole('button', { name, exact: false }).filter({
    has: page.getByText(name, { exact: true }),
  })

const openGroupMenu = async (name: string) => {
  await userEvent.click(groupLink(name).element(), {
    button: 'right',
    force: true,
  })
  await expect.element(page.getByRole('menu')).toBeVisible()
}

const openFolderMenu = async (name: string) => {
  await userEvent.click(folderHeading(name).element(), {
    button: 'right',
    force: true,
  })
  await expect.element(page.getByRole('menu')).toBeVisible()
}

const closeMenu = () =>
  expect.element(page.getByRole('menu')).not.toBeInTheDocument()

const names = (view: Awaited<ReturnType<typeof renderAt>>['view']) =>
  Array.from(view.container.querySelectorAll('a')).map(
    (link) => link.textContent?.match(/Alpha|Bravo|Charlie|Delta/)?.[0],
  )

test('the groups show in the stored order, a new one last, folders after', async () => {
  const { view } = await renderAt('/main/proxies/group/Alpha', {
    stored: arranged,
  })

  expect(names(view)).toEqual(['Charlie', 'Alpha', 'Bravo', 'Delta'])

  for (const heading of [m.proxies_nav_groups(), m.proxies_nav_folders()]) {
    await expect
      .element(view.getByRole('heading', { name: heading, exact: true }))
      .toBeInTheDocument()
  }
  await expect
    .element(
      folderHeading('Work').getByText(m.proxies_nav_group_count({ count: 1 })),
    )
    .toBeInTheDocument()
})

test('without a stored arrangement the groups keep the core order', async () => {
  const { view } = await renderAt('/main/proxies/group/Alpha')

  expect(names(view)).toEqual(['Alpha', 'Bravo', 'Charlie', 'Delta'])
})

test('an item shows its type, pin, current node and delay', async () => {
  await renderAt('/main/proxies/group/Alpha', { stored: arranged })

  const alpha = groupLink('Alpha')
  await expect.element(alpha.getByText('Selector')).toBeInTheDocument()
  await expect.element(alpha.getByText('hk-1')).toBeInTheDocument()
  await expect.element(alpha.getByText('120 ms')).toBeInTheDocument()
  expect(
    alpha.element().querySelector('[data-slot="group-summary-fixed-icon"]'),
  ).not.toBeNull()
  await expect.element(alpha).toHaveAttribute('aria-current', 'page')

  // A failed test shows a mark, an untested node none, and only a pinned group
  // has the pin.
  const bravo = groupLink('Bravo')
  await expect.element(bravo.getByText('∞')).toBeInTheDocument()
  expect(
    bravo.element().querySelector('[data-slot="group-summary-fixed-icon"]'),
  ).toBeNull()
  expect(groupLink('Delta').element().textContent).not.toMatch(/ms|∞/)
  await expect.element(bravo).not.toHaveAttribute('aria-current')
})

test('a group picture is shown as it is and its placeholder stays round', async () => {
  await renderAt('/main/proxies/group/Alpha', { stored: arranged })

  const icon = groupLink('Bravo')
    .element()
    .querySelector('[data-slot="navigation-list-item-icon"]')

  expect(icon?.querySelector('img')).not.toBeNull()
  expect(icon?.querySelector('[data-slot="material-shape"]')).toBeNull()
  expect(icon?.querySelector('div')?.className).toContain('rounded-full')
  // A group without a picture keeps its tonal glyph in a shape.
  expect(
    groupLink('Alpha').element().querySelector('[data-slot="material-shape"]'),
  ).not.toBeNull()
})

test('a folder heading opens and closes the folder and remembers it', async () => {
  const { writes } = await renderAt('/main/proxies/group/Alpha', {
    stored: arranged,
  })

  await expect.element(groupLink('Delta')).toBeInTheDocument()
  await expect
    .element(folderHeading('Work'))
    .toHaveAttribute('aria-expanded', 'true')

  await folderHeading('Work').click()

  await expect
    .element(folderHeading('Work'))
    .toHaveAttribute('aria-expanded', 'false')
  await expect.element(groupLink('Delta')).not.toBeInTheDocument()
  await expect.poll(() => writes).toContainEqual({ f1: true })
})

test('opening a group in a closed folder opens the folder', async () => {
  await renderAt('/main/proxies/group/Delta', {
    stored: arranged,
    collapsed: { f1: true },
  })

  await expect.element(groupLink('Delta')).toBeInTheDocument()
  await expect
    .element(folderHeading('Work'))
    .toHaveAttribute('aria-expanded', 'true')
})

test('an empty folder says where groups go', async () => {
  await renderAt('/main/proxies/group/Alpha', {
    stored: {
      folders: [{ id: 'f1', name: 'Work', members: [] }],
      ungrouped: ['Alpha', 'Bravo', 'Charlie', 'Delta'],
    },
  })

  await expect
    .element(page.getByText(m.proxies_nav_folder_empty()))
    .toBeInTheDocument()
})

test('a group moves into a folder in one commit', async () => {
  const { layouts, view } = await renderAt('/main/proxies/group/Alpha', {
    stored: arranged,
  })

  await openGroupMenu('Alpha')
  await page.getByRole('menuitem', { name: m.profile_move_to_folder() }).click()
  await page.getByRole('menuitem', { name: 'Work' }).click()

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0]).toEqual({
    folders: [{ id: 'f1', name: 'Work', members: ['Delta', 'Alpha'] }],
    // Bravo was placed by the sidebar, so the commit stores it too.
    ungrouped: ['Charlie', 'Bravo'],
  })
  await expect
    .poll(() => names(view))
    .toEqual(['Charlie', 'Bravo', 'Delta', 'Alpha'])
})

test('a group leaves its folder from its menu', async () => {
  const { layouts } = await renderAt('/main/proxies/group/Alpha', {
    stored: arranged,
  })

  await openGroupMenu('Delta')
  await page.getByRole('menuitem', { name: m.profile_move_to_folder() }).click()
  await page
    .getByRole('menuitem', { name: m.profile_move_remove_from_folder() })
    .click()

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].folders).toEqual([{ id: 'f1', name: 'Work', members: [] }])
  expect(layouts[0].ungrouped).toEqual(['Charlie', 'Alpha', 'Bravo', 'Delta'])
})

test('a group moves into a new folder', async () => {
  const { created } = await renderAt('/main/proxies/group/Alpha', {
    stored: arranged,
  })

  await openGroupMenu('Alpha')
  await page.getByRole('menuitem', { name: m.profile_move_to_folder() }).click()
  await page
    .getByRole('menuitem', { name: m.profile_move_new_folder() })
    .click()
  await page
    .getByRole('textbox', { name: m.profile_folder_name_label() })
    .fill('Games')
  await page.getByRole('button', { name: m.common_save() }).click()

  await expect.poll(() => created).toEqual([['Games', ['Alpha']]])
})

test('Move up and Move down reorder a group in one commit', async () => {
  const { layouts } = await renderAt('/main/proxies/group/Alpha', {
    stored: arranged,
  })

  await openGroupMenu('Bravo')
  await page.getByRole('menuitem', { name: m.profile_folder_move_up() }).click()

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].ungrouped).toEqual(['Charlie', 'Bravo', 'Alpha'])
  await closeMenu()

  await openGroupMenu('Charlie')
  await page
    .getByRole('menuitem', { name: m.profile_folder_move_down() })
    .click()

  await expect.poll(() => layouts).toHaveLength(2)
  expect(layouts[1].ungrouped).toEqual(['Bravo', 'Charlie', 'Alpha'])
})

test('the first group cannot move up and the last cannot move down', async () => {
  await renderAt('/main/proxies/group/Alpha', { stored: arranged })

  await openGroupMenu('Charlie')
  await expect
    .element(page.getByRole('menuitem', { name: m.profile_folder_move_up() }))
    .toHaveAttribute('aria-disabled', 'true')
  await userEvent.keyboard('{Escape}')
  await closeMenu()

  await openGroupMenu('Bravo')
  await expect
    .element(page.getByRole('menuitem', { name: m.profile_folder_move_down() }))
    .toHaveAttribute('aria-disabled', 'true')
})

test('Reset order asks, then commits the empty arrangement', async () => {
  const { layouts } = await renderAt('/main/proxies/group/Alpha', {
    stored: arranged,
  })
  vi.mocked(ask).mockResolvedValueOnce(false).mockResolvedValueOnce(true)

  await page.getByRole('button', { name: m.proxies_nav_list_actions() }).click()
  await page
    .getByRole('menuitem', { name: m.proxies_nav_reset_order() })
    .click()
  await expect.poll(() => vi.mocked(ask).mock.calls).toHaveLength(1)
  expect(layouts).toEqual([])

  await page.getByRole('button', { name: m.proxies_nav_list_actions() }).click()
  await page
    .getByRole('menuitem', { name: m.proxies_nav_reset_order() })
    .click()

  await expect.poll(() => layouts).toEqual([emptyLayout])
})

test('Reset order is unavailable while nothing is arranged', async () => {
  await renderAt('/main/proxies/group/Alpha')

  await page.getByRole('button', { name: m.proxies_nav_list_actions() }).click()
  await expect
    .element(page.getByRole('menuitem', { name: m.proxies_nav_reset_order() }))
    .toHaveAttribute('aria-disabled', 'true')
})

test('Collapse all and Expand all fold every folder', async () => {
  const { writes } = await renderAt('/main/proxies/group/Alpha', {
    stored: arranged,
  })

  await page.getByRole('button', { name: m.proxies_nav_list_actions() }).click()
  await page
    .getByRole('menuitem', { name: m.proxies_nav_collapse_all() })
    .click()

  await expect
    .element(folderHeading('Work'))
    .toHaveAttribute('aria-expanded', 'false')
  await expect.poll(() => writes).toContainEqual({ f1: true })

  await page.getByRole('button', { name: m.proxies_nav_list_actions() }).click()
  await page.getByRole('menuitem', { name: m.proxies_nav_expand_all() }).click()

  await expect
    .element(folderHeading('Work'))
    .toHaveAttribute('aria-expanded', 'true')
})

test('the add button creates a folder', async () => {
  const { created } = await renderAt('/main/proxies/group/Alpha')

  await page.getByRole('button', { name: m.profile_folder_new_title() }).click()
  await page
    .getByRole('textbox', { name: m.profile_folder_name_label() })
    .fill('Games')
  await page.getByRole('button', { name: m.common_save() }).click()

  await expect.poll(() => created).toEqual([['Games', []]])
})

test('a folder is renamed from its menu', async () => {
  const { layouts } = await renderAt('/main/proxies/group/Alpha', {
    stored: arranged,
  })

  await openFolderMenu('Work')
  await page
    .getByRole('menuitem', { name: m.profile_folder_rename_title() })
    .click()

  const input = page.getByRole('textbox', {
    name: m.profile_folder_name_label(),
  })
  await expect.element(input).toHaveValue('Work')
  await input.fill('Office')
  await page.getByRole('button', { name: m.common_save() }).click()

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].folders.map((folder) => folder.name)).toEqual(['Office'])
  await expect.element(folderHeading('Office')).toBeInTheDocument()
})

test('deleting a folder with groups asks first; an empty one does not', async () => {
  const { layouts } = await renderAt('/main/proxies/group/Alpha', {
    stored: {
      folders: [
        { id: 'f1', name: 'Work', members: ['Delta'] },
        { id: 'f2', name: 'Home', members: [] },
      ],
      ungrouped: ['Charlie', 'Alpha', 'Bravo'],
    },
  })
  vi.mocked(ask).mockReset()
  vi.mocked(ask).mockResolvedValueOnce(false)

  await openFolderMenu('Work')
  await page
    .getByRole('menuitem', { name: m.profile_folder_delete_title() })
    .click()
  await expect.poll(() => vi.mocked(ask).mock.calls).toHaveLength(1)
  expect(layouts).toEqual([])
  await closeMenu()

  await openFolderMenu('Home')
  await page
    .getByRole('menuitem', { name: m.profile_folder_delete_title() })
    .click()

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].folders.map((folder) => folder.id)).toEqual(['f1'])
  expect(vi.mocked(ask).mock.calls).toHaveLength(1)
})

test('deleting a folder keeps its groups, at the end of the list', async () => {
  const { layouts } = await renderAt('/main/proxies/group/Alpha', {
    stored: arranged,
  })
  vi.mocked(ask).mockReset()
  vi.mocked(ask).mockResolvedValueOnce(true)

  await openFolderMenu('Work')
  await page
    .getByRole('menuitem', { name: m.profile_folder_delete_title() })
    .click()

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0]).toEqual({
    folders: [],
    ungrouped: ['Charlie', 'Alpha', 'Bravo', 'Delta'],
  })
})

test('a failed commit shows an error dialog and keeps the order', async () => {
  const { view } = await renderAt('/main/proxies/group/Alpha', {
    stored: arranged,
    failLayout: true,
  })

  await openGroupMenu('Bravo')
  await page.getByRole('menuitem', { name: m.profile_folder_move_up() }).click()

  await expect.poll(() => vi.mocked(message).mock.calls).toHaveLength(1)
  expect(vi.mocked(message).mock.calls[0][1]).toMatchObject({
    kind: 'error',
    title: m.proxies_layout_error_title(),
  })
  await expect
    .poll(() => names(view))
    .toEqual(['Charlie', 'Alpha', 'Bravo', 'Delta'])
})

// Dragging.

const emptyFolderLayout: ProxyGroupLayout = {
  folders: [
    { id: 'f1', name: 'Work', members: ['Delta'] },
    { id: 'f2', name: 'Home', members: [] },
  ],
  ungrouped: ['Charlie', 'Alpha', 'Bravo'],
}

test('dragging a group over another reorders the groups in one commit', async () => {
  const { layouts } = await renderAt('/main/proxies/group/Alpha', {
    stored: emptyFolderLayout,
  })

  await drag(groupLink('Bravo').element(), groupLink('Charlie').element())

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].ungrouped).toEqual(['Bravo', 'Charlie', 'Alpha'])
  expect(layouts[0].folders.map((folder) => folder.members)).toEqual([
    ['Delta'],
    [],
  ])
})

test('dropping a group on a folder heading appends it to the folder', async () => {
  const { layouts } = await renderAt('/main/proxies/group/Alpha', {
    stored: emptyFolderLayout,
  })

  await drag(groupLink('Alpha').element(), folderHeading('Work').element())

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].folders[0].members).toEqual(['Delta', 'Alpha'])
  expect(layouts[0].ungrouped).toEqual(['Charlie', 'Bravo'])
})

test('a closed folder takes a group dropped on its heading', async () => {
  const { layouts } = await renderAt('/main/proxies/group/Alpha', {
    stored: emptyFolderLayout,
    collapsed: { f1: true },
  })

  await drag(groupLink('Bravo').element(), folderHeading('Work').element())

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].folders[0].members).toEqual(['Delta', 'Bravo'])
  await expect
    .element(folderHeading('Work'))
    .toHaveAttribute('aria-expanded', 'false')
})

test("dropping a group among a folder's groups puts it in that place", async () => {
  const { layouts } = await renderAt('/main/proxies/group/Alpha', {
    stored: emptyFolderLayout,
  })

  await drag(groupLink('Bravo').element(), groupLink('Delta').element())

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].folders[0].members).toHaveLength(2)
  expect(layouts[0].folders[0].members).toContain('Bravo')
  expect(layouts[0].ungrouped).toEqual(['Charlie', 'Alpha'])
})

test('a group dragged out of a folder leaves it', async () => {
  const { layouts } = await renderAt('/main/proxies/group/Alpha', {
    stored: emptyFolderLayout,
  })

  await drag(groupLink('Delta').element(), groupLink('Alpha').element())

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].folders[0].members).toEqual([])
  expect(layouts[0].ungrouped).toHaveLength(4)
  expect(layouts[0].ungrouped).toContain('Delta')
})

test('an empty folder takes a group dropped on it', async () => {
  const { layouts } = await renderAt('/main/proxies/group/Alpha', {
    stored: emptyFolderLayout,
  })

  await drag(
    groupLink('Charlie').element(),
    page.getByText(m.proxies_nav_folder_empty()).element(),
  )

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].folders[1].members).toEqual(['Charlie'])
  expect(layouts[0].ungrouped).toEqual(['Alpha', 'Bravo'])
})

test('dragging a folder over another reorders the folders in one commit', async () => {
  const { layouts } = await renderAt('/main/proxies/group/Alpha', {
    stored: emptyFolderLayout,
  })

  await drag(folderHeading('Home').element(), folderHeading('Work').element())

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].folders.map((folder) => folder.id)).toEqual(['f2', 'f1'])
  expect(layouts[0].folders.map((folder) => folder.members)).toEqual([
    [],
    ['Delta'],
  ])
})

test('a drop in the same place commits nothing', async () => {
  const { layouts } = await renderAt('/main/proxies/group/Alpha', {
    stored: emptyFolderLayout,
  })

  await drag(groupLink('Alpha').element(), groupLink('Alpha').element())

  expect(layouts).toEqual([])
})

test('a click on a group still opens it and a click on a folder toggles it', async () => {
  const { layouts } = await renderAt('/main/proxies/group/Alpha', {
    stored: emptyFolderLayout,
  })

  await groupLink('Bravo').click()
  await expect
    .element(groupLink('Bravo'))
    .toHaveAttribute('aria-current', 'page')

  await folderHeading('Work').click()
  await expect
    .element(folderHeading('Work'))
    .toHaveAttribute('aria-expanded', 'false')
  expect(layouts).toEqual([])
})

test('a group moves with the keyboard', async () => {
  const { layouts } = await renderAt('/main/proxies/group/Alpha', {
    stored: emptyFolderLayout,
  })

  groupLink('Charlie').element().focus()
  await userEvent.keyboard(' ')
  await userEvent.keyboard('{ArrowDown}')
  await userEvent.keyboard(' ')

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].ungrouped).toEqual(['Alpha', 'Charlie', 'Bravo'])
})

test('escape cancels a drag without a commit and the item can be picked up again', async () => {
  const { layouts, view } = await renderAt('/main/proxies/group/Alpha', {
    stored: emptyFolderLayout,
  })

  groupLink('Charlie').element().focus()
  await userEvent.keyboard(' ')
  await userEvent.keyboard('{ArrowDown}')
  await userEvent.keyboard('{Escape}')

  await expect
    .poll(() => names(view))
    .toEqual(['Charlie', 'Alpha', 'Bravo', 'Delta'])
  expect(layouts).toEqual([])

  // The same item can be picked up again after the cancel.
  await expect
    .poll(() => document.querySelector('[data-dnd-dragging]'))
    .toBeNull()
  groupLink('Charlie').element().focus()
  await userEvent.keyboard(' ')
  await userEvent.keyboard('{ArrowDown}')
  await userEvent.keyboard(' ')

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].ungrouped).toEqual(['Alpha', 'Charlie', 'Bravo'])
})
