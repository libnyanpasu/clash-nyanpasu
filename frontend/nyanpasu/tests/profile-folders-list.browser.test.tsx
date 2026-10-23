import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { page, userEvent } from 'vitest/browser'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { SnackbarProvider } from '@nyanpasu/ui/snackbar'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { ImportUrlProvider } from '@/components/profile-import/import-url-provider'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import ContextMenuProvider from '@/components/providers/context-menu-provider'
import ProfilesDndProvider from '@/pages/(main)/main/profiles/_modules/profiles-dnd-provider'
import ProfilesLayoutProvider from '@/pages/(main)/main/profiles/_modules/profiles-layout-provider'
import ProfilesNavigate from '@/pages/(main)/main/profiles/_modules/profiles-navigate'
import ProfilesList from '@/pages/(main)/main/profiles/$type/_modules/profiles-list'
import { m } from '@/paraglide/messages'
import { ask } from '@/utils/notification'
import type {
  ProfileFolder,
  ProfileItem_Serialize,
  ProfileLayout,
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
vi.mock('@/pages/(main)/main/profiles/$type/index', () => ({
  Route: { useParams: () => ({ type: 'profile' }) },
}))
vi.mock('@/utils/notification', () => ({ message: vi.fn(), ask: vi.fn() }))
vi.mock('@/components/providers/theme-provider', async () => {
  const { argbFromHex, themeFromSourceColor } =
    await import('@material/material-color-utilities')
  return {
    useExperimentalThemeContext: () => ({
      themePalette: themeFromSourceColor(argbFromHex('#1867C0')),
    }),
  }
})

const COLLAPSED_KEY = 'profiles-folder-collapsed'

const config = (uid: string): ProfileItem_Serialize => ({
  type: 'config',
  uid,
  name: uid,
  config: {
    type: 'file',
    transforms: [],
    source: {
      type: 'local',
      binding: { type: 'managed', file: `${uid}.yaml` },
    },
  },
})

const committed = (value: unknown = null) => ({
  status: 'committed',
  value,
  commits: [],
})

const folders = (): ProfileFolder[] => [
  { id: 'f1', name: 'Work', members: ['a', 'b'] },
  { id: 'f2', name: 'Home', members: ['c'] },
  { id: 'f3', name: 'Travel', members: [] },
]

const renderPage = async ({
  initialFolders = folders(),
  storage = {},
}: {
  initialFolders?: ProfileFolder[]
  storage?: Record<string, unknown>
} = {}) => {
  // Keep every section in view: the pointer helper must not start a drag
  // below the viewport or race the auto-scroller on its way to the sidebar.
  await page.viewport(1400, 1800)

  const ungrouped = ['d', 'e']
  let document = {
    current: 'a',
    valid: [],
    global_transforms: [],
    items: [
      ...initialFolders.flatMap((folder) => folder.members),
      ...(initialFolders.length ? ungrouped : ['a', 'b', 'c', 'd', 'e']),
    ].map(config),
    folders: initialFolders,
  }
  const layouts: ProfileLayout[] = []
  const creations: unknown[] = []
  const writes: { key: string; value: unknown }[] = []

  mockIPC(
    (command, args) => {
      const wire = args as {
        method: string
        params: Record<string, unknown>
      }
      const method = command === 'call_rpc' ? wire.method : command

      if (method === 'get_storage_item') {
        const key = String(wire.params.key)

        return key in storage ? JSON.stringify(storage[key]) : null
      }
      if (method === 'set_storage_item') {
        writes.push({
          key: String(wire.params.key),
          value: JSON.parse(String(wire.params.value)),
        })

        return committed()
      }
      if (method === 'get_profiles') return document
      if (method === 'set_profile_layout') {
        const layout = wire.params.layout as ProfileLayout
        layouts.push(layout)
        document = {
          ...document,
          folders: layout.folders,
          items: [
            ...layout.ungrouped,
            ...layout.folders.flatMap((folder) => folder.members),
          ].map(config),
        }

        return committed()
      }
      if (method === 'create_profile_folder') {
        creations.push([wire.params.name, wire.params.members])

        return committed('f9')
      }
      if (method === 'read_profile_file') return 'proxies: []'
      if (method === 'query_traffic_report')
        return {
          total: { bytes: { upload: 0, download: 0 }, connections: 0 },
          current_rate: null,
          rankings: [],
          topology: null,
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
            <ImportUrlProvider>
              <SnackbarProvider>
                <ContextMenuProvider>
                  <div>
                    <ProfilesLayoutProvider>
                      <ProfilesDndProvider>
                        <div className="flex h-[1700px]">
                          <aside
                            className="w-72 shrink-0"
                            data-testid="sidebar"
                          >
                            <ProfilesNavigate />
                          </aside>

                          <main
                            className="min-w-0 flex-1"
                            data-testid="content"
                          >
                            <ScrollArea>
                              <ProfilesList className="p-4 pt-0" />
                            </ScrollArea>
                          </main>
                        </div>
                      </ProfilesDndProvider>
                    </ProfilesLayoutProvider>
                  </div>
                </ContextMenuProvider>
              </SnackbarProvider>
            </ImportUrlProvider>
          </TooltipProvider>
        </BlockTaskProvider>
      </TestQueryProvider>
    ),
  })
  const router = createRouter({
    routeTree: rootRoute.addChildren([
      createRoute({ getParentRoute: () => rootRoute, path: '/' }),
    ]),
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })

  const view = await render(<RouterProvider router={router} />)

  await expect
    .poll(() => view.container.querySelector('[data-profile-uid="e"]'))
    .not.toBeNull()

  const card = (uid: string) =>
    view.container.querySelector<HTMLElement>(`[data-profile-uid="${uid}"]`)!

  // The grab point of a card: its title, not one of its buttons.
  const body = (uid: string) =>
    card(uid).querySelector<HTMLElement>('[data-slot="profile-card-title"]')!

  const section = (folder: string | null) =>
    view.container.querySelector<HTMLElement>(
      folder
        ? `[data-slot="profiles-folder-section"][data-folder="${folder}"]`
        : '[data-slot="profiles-folder-section"]:not([data-folder])',
    )!

  const uidsIn = (folder: string | null) =>
    Array.from(
      section(folder).querySelectorAll<HTMLElement>('[data-profile-uid]'),
    ).map((element) => element.dataset.profileUid)

  const folderLink = (name: string) =>
    view.container.querySelector<HTMLAnchorElement>(
      `[data-testid="sidebar"] a[href$="${
        initialFolders.find((folder) => folder.name === name)?.id
      }"]`,
    )!

  return {
    view,
    router,
    layouts,
    creations,
    writes,
    card,
    body,
    section,
    uidsIn,
    folderLink,
  }
}

test('profiles are listed by folder, then those in none', async () => {
  const { view, uidsIn, section, card } = await renderPage()

  const headings = Array.from(
    view.container.querySelectorAll(
      '[data-slot="profiles-folder-section-heading"]',
    ),
  ).map((heading) => heading.textContent)
  expect(headings).toEqual([
    'Work1',
    'Home1',
    'Travel0',
    `${m.profile_folder_section_ungrouped()}2`,
  ])

  // The active profile `a` is a member of Work but listed apart.
  expect(uidsIn('f1')).toEqual(['b'])
  expect(uidsIn('f2')).toEqual(['c'])
  expect(uidsIn('f3')).toEqual([])
  expect(uidsIn(null)).toEqual(['d', 'e'])
  expect(
    section('f3').querySelector('[data-slot="profiles-folder-section-empty"]')
      ?.textContent,
  ).toBe(m.profile_folder_empty_hint())

  // The active profile is the featured card, above every folder.
  expect(card('a').dataset.active).toBe('true')
  const active = view.container.querySelector('[data-slot="profiles-active"]')!
  expect(active.contains(card('a'))).toBe(true)
  expect(
    active.compareDocumentPosition(section('f1')) &
      Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy()
  expect(section('f1').contains(card('a'))).toBe(false)
})

test('without folders the list is a plain grid with no heading', async () => {
  const { view } = await renderPage({ initialFolders: [] })

  expect(
    view.container.querySelectorAll(
      '[data-slot="profiles-folder-section-heading"]',
    ),
  ).toHaveLength(0)
  expect(view.container.querySelectorAll('[data-profile-uid]')).toHaveLength(5)
  expect(
    view.container.querySelectorAll('[data-slot="profiles-grid"]'),
  ).toHaveLength(1)
  expect(
    view.container.querySelector('[data-slot="profiles-active"]'),
  ).not.toBeNull()
})

test('a section collapses and the choice is stored per folder', async () => {
  const { view, section, writes, uidsIn } = await renderPage({
    storage: { [COLLAPSED_KEY]: { f2: true } },
  })

  // Home was stored as collapsed.
  await expect.poll(() => uidsIn('f2')).toEqual([])
  expect(
    section('f2')
      .querySelector('[data-slot="profiles-folder-section-toggle"]')
      ?.getAttribute('aria-expanded'),
  ).toBe('false')
  expect(uidsIn('f1')).toEqual(['b'])

  section('f1')
    .querySelector<HTMLElement>('[data-slot="profiles-folder-section-toggle"]')!
    .click()

  await expect.poll(() => uidsIn('f1')).toEqual([])
  expect(writes).toEqual([
    { key: COLLAPSED_KEY, value: { f2: true, f1: true } },
  ])
  expect(
    view.container.querySelector('[data-slot="profiles-folder-section"]'),
  ).not.toBeNull()
})

test('a card dropped on another section moves there in one commit', async () => {
  const { layouts, body, card, section, uidsIn } = await renderPage()

  await drag(body('d'), card('b'))

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].folders.map((folder) => folder.members)).toEqual([
    expect.arrayContaining(['a', 'b', 'd']),
    ['c'],
    [],
  ])
  expect(layouts[0].ungrouped).toEqual(['e'])
  await expect.poll(() => uidsIn('f1')).toHaveLength(2)
  expect(section('f1').contains(card('d'))).toBe(true)
  expect(layouts).toHaveLength(1)
})

test('an empty folder takes a card dropped on its zone', async () => {
  const { layouts, body, section } = await renderPage()

  await drag(
    body('e'),
    section('f3').querySelector<HTMLElement>(
      '[data-slot="profiles-folder-section-empty"]',
    )!,
  )

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].folders.map((folder) => folder.members)).toEqual([
    ['a', 'b'],
    ['c'],
    ['e'],
  ])
  expect(layouts[0].ungrouped).toEqual(['d'])
})

test('the active card is not dragged and takes no drop', async () => {
  const { layouts, body, card, uidsIn } = await renderPage()

  await drag(body('a'), card('d'))
  expect(layouts).toEqual([])
  expect(uidsIn(null)).toEqual(['d', 'e'])

  await drag(body('d'), card('a'))
  await expect.poll(() => uidsIn(null)).toEqual(['d', 'e'])
  expect(layouts).toEqual([])
})

test('dropping a card on a sidebar folder moves it once, without moving its element', async () => {
  const { router, layouts, body, card, uidsIn, folderLink, view } =
    await renderPage()

  const sidebar = view.container.querySelector('[data-testid="sidebar"]')!
  const intruders: string[] = []
  const observer = new MutationObserver((records) => {
    for (const record of records) {
      for (const node of record.addedNodes) {
        if (
          node instanceof Element &&
          (node.matches('[data-profile-uid]') ||
            node.querySelector('[data-profile-uid]'))
        ) {
          intruders.push(node.getAttribute('data-profile-uid') ?? 'child')
        }
      }
    }
  })
  observer.observe(sidebar, { childList: true, subtree: true })

  await drag(body('d'), folderLink('Home'))

  await expect.poll(() => layouts).toHaveLength(1)
  observer.disconnect()

  expect(layouts[0].folders.map((folder) => folder.members)).toEqual([
    ['a', 'b'],
    ['c', 'd'],
    [],
  ])
  expect(layouts[0].ungrouped).toEqual(['e'])
  expect(intruders).toEqual([])
  expect(sidebar.contains(card('d'))).toBe(false)
  await expect.poll(() => uidsIn('f2')).toEqual(['c', 'd'])

  // Releasing over a link is not a click on it.
  expect(router.state.location.pathname).toBe('/')
  expect(layouts).toHaveLength(1)
})

test('dropping a card on All profiles takes it out of its folder', async () => {
  const { layouts, body, view, uidsIn } = await renderPage()

  await drag(
    body('c'),
    view.container.querySelector<HTMLElement>(
      '[data-slot="profiles-all-nav-item"]',
    )!,
  )

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].folders.map((folder) => folder.members)).toEqual([
    ['a', 'b'],
    [],
    [],
  ])
  expect(layouts[0].ungrouped).toEqual(['d', 'e', 'c'])
  await expect.poll(() => uidsIn(null)).toEqual(['d', 'e', 'c'])
})

const announcement = () =>
  Array.from(document.querySelectorAll('[role="status"]'))
    .map((region) => region.textContent)
    .join(' ')

test('a card moves with the keyboard and the move is announced', async () => {
  const { layouts, card } = await renderPage()

  card('d').focus()
  await userEvent.keyboard(' ')

  // The pickup is followed at once by where the card now stands.
  await expect.poll(announcement).toBe(
    m.profile_folder_announce_move({
      profile: 'd',
      folder: m.profile_folder_section_ungrouped(),
      position: 1,
      total: 2,
    }),
  )

  await userEvent.keyboard('{ArrowUp}')
  await userEvent.keyboard(' ')

  await expect
    .poll(announcement)
    .toContain(m.profile_folder_announce_drop({ name: 'd' }))
  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].ungrouped).not.toContain('d')
})

test('Escape cancels a keyboard move without a commit', async () => {
  const { layouts, card } = await renderPage()

  card('d').focus()
  await userEvent.keyboard(' ')
  await userEvent.keyboard('{ArrowUp}')
  await userEvent.keyboard('{Escape}')

  await expect
    .poll(announcement)
    .toContain(m.profile_folder_announce_cancel({ name: 'd' }))
  expect(layouts).toEqual([])
})

test('a card moves into a folder from its context menu', async () => {
  const { layouts, card, uidsIn } = await renderPage()

  await userEvent.click(card('d'), { button: 'right', force: true })
  await page.getByRole('menuitem', { name: m.profile_move_to_folder() }).click()
  await page.getByRole('menuitem', { name: 'Home' }).click()

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].folders.map((folder) => folder.members)).toEqual([
    ['a', 'b'],
    ['c', 'd'],
    [],
  ])
  expect(layouts[0].ungrouped).toEqual(['e'])
  await expect.poll(() => uidsIn('f2')).toEqual(['c', 'd'])
})

test('a card leaves its folder from its context menu', async () => {
  const { layouts, card, uidsIn } = await renderPage()

  await userEvent.click(card('c'), { button: 'right', force: true })
  await page.getByRole('menuitem', { name: m.profile_move_to_folder() }).click()
  // Its own folder is shown but cannot be picked again.
  await expect
    .element(page.getByRole('menuitem', { name: 'Home' }))
    .toHaveAttribute('data-disabled')
  await page
    .getByRole('menuitem', { name: m.profile_move_remove_from_folder() })
    .click()

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].folders.map((folder) => folder.members)).toEqual([
    ['a', 'b'],
    [],
    [],
  ])
  expect(layouts[0].ungrouped).toEqual(expect.arrayContaining(['c', 'd', 'e']))
  await expect.poll(() => uidsIn(null)).toContain('c')
})

test('a card moves into a new folder from its context menu', async () => {
  const { creations, card } = await renderPage()

  await userEvent.click(card('e'), { button: 'right', force: true })
  await page.getByRole('menuitem', { name: m.profile_move_to_folder() }).click()
  await page
    .getByRole('menuitem', { name: m.profile_move_new_folder() })
    .click()

  const input = page.getByRole('textbox', {
    name: m.profile_folder_name_label(),
  })
  await input.fill('Games')
  await page.getByRole('button', { name: m.common_save() }).click()

  await expect.poll(() => creations).toEqual([['Games', ['e']]])
})

test('a section heading renames and deletes its folder', async () => {
  vi.mocked(ask).mockResolvedValue(true)

  const { layouts, section } = await renderPage()
  const actions = (folder: string) =>
    section(folder).querySelector<HTMLElement>(
      '[data-slot="profiles-folder-actions"]',
    )!

  // The group of profiles in no folder has no menu.
  expect(
    section(null).querySelector('[data-slot="profiles-folder-actions"]'),
  ).toBeNull()

  await userEvent.click(actions('f2'))
  await page
    .getByRole('menuitem', { name: m.profile_folder_rename_title() })
    .click()
  const input = page.getByRole('textbox', {
    name: m.profile_folder_name_label(),
  })
  await input.fill('House')
  await page.getByRole('button', { name: m.common_save() }).click()

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].folders.map((folder) => folder.name)).toEqual([
    'Work',
    'House',
    'Travel',
  ])

  await expect.poll(() => actions('f3')).not.toBeNull()
  await userEvent.click(actions('f3'))
  await page
    .getByRole('menuitem', { name: m.profile_folder_delete_title() })
    .click()

  await expect.poll(() => layouts).toHaveLength(2)
  expect(layouts[1].folders.map((folder) => folder.id)).toEqual(['f1', 'f2'])
})
