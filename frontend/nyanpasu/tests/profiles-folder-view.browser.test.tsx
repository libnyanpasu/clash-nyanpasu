import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { page, userEvent } from 'vitest/browser'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { SharedTooltip } from '@nyanpasu/ui/shared-tooltip'
import { SnackbarProvider } from '@nyanpasu/ui/snackbar'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { ImportUrlProvider } from '@/components/profile-import/import-url-provider'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import ContextMenuProvider from '@/components/providers/context-menu-provider'
import ProfilesDndProvider from '@/pages/(main)/main/profiles/_modules/profiles-dnd-provider'
import ProfilesLayoutProvider from '@/pages/(main)/main/profiles/_modules/profiles-layout-provider'
import { Route as FolderRoute } from '@/pages/(main)/main/profiles/folder/$folderId/index'
import { m } from '@/paraglide/messages'
import { ask } from '@/utils/notification'
import type { ProfileItem_Serialize, ProfileLayout } from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet as RouterOutlet,
  RouterProvider,
} from '@tanstack/react-router'
import { mockIPC } from '@tauri-apps/api/mocks'
import { TestQueryProvider } from './query-provider'

vi.hoisted(() => {
  vi.stubGlobal('isTauri', true)
})
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

const renderFolder = async (folderId: string) => {
  await page.viewport(1200, 900)

  let document = {
    current: 'a',
    valid: [],
    global_transforms: [],
    items: ['a', 'b', 'c', 'd'].map(config),
    folders: [
      { id: 'f1', name: 'Work', members: ['a', 'b'] },
      { id: 'f2', name: 'Home', members: ['c'] },
      { id: 'f3', name: 'Travel', members: [] },
    ],
  }
  const layouts: ProfileLayout[] = []
  const imports: Record<string, unknown>[] = []

  mockIPC(
    (command, args) => {
      const wire = args as { method: string; params: Record<string, unknown> }
      const method = command === 'call_rpc' ? wire.method : command

      if (method === 'get_storage_item') return { value: null, revision: 0 }
      if (method === 'get_profiles') return document
      if (method === 'set_profile_layout') {
        const layout = wire.params.layout as ProfileLayout
        layouts.push(layout)
        document = { ...document, folders: layout.folders }

        return committed()
      }
      if (method === 'probe_profile_url')
        return {
          classification: { suggested: 'config', evidence: [] },
          accepted: ['config', 'overlay', 'javascript', 'lua'],
          suggested_name: 'profile',
          subscription: {},
          suggested_update_interval_minutes: null,
          content: 'proxies: []\n',
        }
      if (method === 'import_profile') {
        imports.push(wire.params)

        return committed('imported')
      }
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
            <SharedTooltip>
              <ImportUrlProvider>
                <SnackbarProvider>
                  <ContextMenuProvider>
                    <div>
                      <ProfilesLayoutProvider>
                        <ProfilesDndProvider>
                          <div className="relative h-[800px]">
                            <ScrollArea>
                              <RouterOutlet />
                            </ScrollArea>
                          </div>
                        </ProfilesDndProvider>
                      </ProfilesLayoutProvider>
                    </div>
                  </ContextMenuProvider>
                </SnackbarProvider>
              </ImportUrlProvider>
            </SharedTooltip>
          </TooltipProvider>
        </BlockTaskProvider>
      </TestQueryProvider>
    ),
  })

  // The file route keeps its own id, which its hooks look up.
  const folderRoute = FolderRoute.update({
    id: '/(main)/main/profiles/folder/$folderId/',
    path: '/folder/$folderId',
    getParentRoute: () => rootRoute,
  } as never)

  const allProfilesRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/main/profiles/$type',
    component: () => <p>All profiles page</p>,
  })

  const router = createRouter({
    routeTree: rootRoute.addChildren([folderRoute, allProfilesRoute]),
    history: createMemoryHistory({
      initialEntries: [`/folder/${folderId}`],
    }),
  })

  const view = await render(<RouterProvider router={router} />)

  const card = (uid: string) =>
    view.container.querySelector<HTMLElement>(`[data-profile-uid="${uid}"]`)

  return { view, router, layouts, imports, card }
}

test('a folder page lists its profiles with the current one first', async () => {
  const { view, card } = await renderFolder('f1')

  await expect.element(view.getByText('Work').first()).toBeInTheDocument()
  await expect.poll(() => card('b')).not.toBeNull()

  // The current profile is featured above the section; `c` and `d` are not
  // members.
  const active = view.container.querySelector('[data-slot="profiles-active"]')!
  expect(active.contains(card('a'))).toBe(true)
  expect(card('a')?.dataset.active).toBe('true')

  const section = view.container.querySelector(
    '[data-slot="profiles-folder-section"][data-folder="f1"]',
  )!
  expect(section.contains(card('b'))).toBe(true)
  expect(section.contains(card('a'))).toBe(false)
  expect(card('c')).toBeNull()
  expect(card('d')).toBeNull()

  // One section, so no heading to collapse.
  expect(
    view.container.querySelector(
      '[data-slot="profiles-folder-section-heading"]',
    ),
  ).toBeNull()
})

test('an empty folder says so', async () => {
  const { view, card } = await renderFolder('f3')

  await expect
    .element(view.getByText(m.profile_folder_empty_title()))
    .toBeVisible()
  expect(card('a')).toBeNull()
})

test('an unknown folder links back to all profiles', async () => {
  const { view, router } = await renderFolder('gone')

  await expect
    .element(view.getByText(m.profile_folder_not_found()))
    .toBeVisible()

  await view.getByRole('link', { name: m.profile_nav_all_profiles() }).click()

  await expect
    .poll(() => router.state.location.pathname)
    .toBe('/main/profiles/profile')
})

test('the overflow menu renames the folder', async () => {
  const { view, layouts } = await renderFolder('f1')

  await view
    .getByRole('button', { name: m.profile_detail_more_actions() })
    .click()
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
  expect(layouts[0].folders.map((folder) => folder.name)).toEqual([
    'Office',
    'Home',
    'Travel',
  ])
  await expect.element(view.getByText('Office').first()).toBeInTheDocument()
})

test('deleting the folder returns to all profiles', async () => {
  vi.mocked(ask).mockResolvedValue(true)

  const { view, layouts, router } = await renderFolder('f1')

  await view
    .getByRole('button', { name: m.profile_detail_more_actions() })
    .click()
  await page
    .getByRole('menuitem', { name: m.profile_folder_delete_title() })
    .click()

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].folders.map((folder) => folder.id)).toEqual(['f2', 'f3'])
  // Its profiles are in no folder now.
  expect(layouts[0].ungrouped).toEqual(expect.arrayContaining(['a', 'b']))
  await expect
    .poll(() => router.state.location.pathname)
    .toBe('/main/profiles/profile')
  await expect.element(view.getByText('All profiles page')).toBeVisible()
})

test('a declined deletion keeps the folder and the page', async () => {
  vi.mocked(ask).mockResolvedValue(false)

  const { view, layouts, router } = await renderFolder('f1')

  await view
    .getByRole('button', { name: m.profile_detail_more_actions() })
    .click()
  await page
    .getByRole('menuitem', { name: m.profile_folder_delete_title() })
    .click()

  await expect.poll(() => vi.mocked(ask).mock.calls.length).toBeGreaterThan(0)
  expect(layouts).toHaveLength(0)
  expect(router.state.location.pathname).toBe('/folder/f1')
})

test('quick import joins the folder', async () => {
  const { view, imports } = await renderFolder('f1')

  const input = view.getByPlaceholder(m.profile_quick_import_placeholder())
  await input.fill('https://example.com/sub')
  await userEvent.keyboard('{Enter}')
  await page
    .getByRole('button', { name: m.profile_import_url_confirm() })
    .click()

  await expect.poll(() => imports).toHaveLength(1)
  expect(imports[0]).toMatchObject({
    url: 'https://example.com/sub',
    folder: 'f1',
  })
})

test('the import button joins the folder', async () => {
  const { view, imports } = await renderFolder('f1')

  await view.getByRole('button', { name: m.profile_import_title() }).click()
  await view.getByRole('menuitem', { name: m.profile_source_remote() }).click()

  await page.getByRole('textbox').nth(2).fill('https://example.com/other')
  await page.getByRole('button', { name: m.common_submit() }).click()
  await page
    .getByRole('button', { name: m.profile_import_url_confirm() })
    .click()

  await expect.poll(() => imports).toHaveLength(1)
  expect(imports[0]).toMatchObject({
    url: 'https://example.com/other',
    folder: 'f1',
  })
})
