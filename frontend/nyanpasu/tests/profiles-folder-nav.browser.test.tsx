import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { page, userEvent } from 'vitest/browser'
import { SnackbarProvider } from '@nyanpasu/ui/snackbar'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import ContextMenuProvider from '@/components/providers/context-menu-provider'
import ProfilesDndProvider from '@/pages/(main)/main/profiles/_modules/profiles-dnd-provider'
import ProfilesLayoutProvider from '@/pages/(main)/main/profiles/_modules/profiles-layout-provider'
import ProfilesNavigate from '@/pages/(main)/main/profiles/_modules/profiles-navigate'
import { m } from '@/paraglide/messages'
import { ask } from '@/utils/notification'
import type { ProfileItem_Serialize, ProfileLayout } from '@nyanpasu/rpc/types'
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

const renderAt = async (path: string) => {
  let document = {
    current: 'a',
    valid: [],
    global_transforms: [],
    items: [config('a'), config('b'), config('c')],
    folders: [
      { id: 'f1', name: 'Work', members: ['a', 'b'] },
      { id: 'f2', name: 'Home', members: ['c'] },
      { id: 'f3', name: 'Travel', members: [] },
    ],
  }
  const layouts: ProfileLayout[] = []
  const created: unknown[] = []

  mockIPC((command, args) => {
    const wire = args as { method: string; params: Record<string, unknown> }
    const method = command === 'call_rpc' ? wire.method : command

    if (method === 'get_profiles') return document
    if (method === 'set_profile_layout') {
      const layout = wire.params.layout as ProfileLayout
      layouts.push(layout)
      document = { ...document, folders: layout.folders }
      return committed()
    }
    if (method === 'create_profile_folder') {
      created.push([wire.params.name, wire.params.members])
      return committed('f9')
    }
    throw new Error(`Unexpected RPC ${method}`)
  })

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
                <div>
                  <ProfilesLayoutProvider>
                    <ProfilesDndProvider>
                      <ProfilesNavigate />
                    </ProfilesDndProvider>
                  </ProfilesLayoutProvider>
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
      createRoute({ getParentRoute: () => rootRoute, path }),
    ]),
    history: createMemoryHistory({ initialEntries: [path] }),
  })

  const view = await render(<RouterProvider router={router} />)

  await expect
    .element(view.getByRole('link', { name: 'Work', exact: false }))
    .toBeInTheDocument()

  return { view, layouts, created }
}

const folderLink = (name: string) =>
  page.getByRole('link', { name, exact: false })

const openMenu = async (name: string) => {
  await userEvent.click(folderLink(name).element(), {
    button: 'right',
    force: true,
  })
  await expect.element(page.getByRole('menu')).toBeVisible()
}

const closeMenu = () =>
  expect.element(page.getByRole('menu')).not.toBeInTheDocument()

test('folders follow All profiles, in folder order, with their counts', async () => {
  const { view } = await renderAt('/main/profiles/profile')

  const labels = Array.from(view.container.querySelectorAll('a')).map(
    (link) => link.textContent,
  )
  expect(labels.slice(0, 4)).toEqual([
    `${m.profile_nav_all_profiles()}${m.profile_nav_count_profiles({ count: 3 })}`,
    `Work${m.profile_nav_count_profiles({ count: 2 })}`,
    `Home${m.profile_nav_count_profiles({ count: 1 })}`,
    `Travel${m.profile_nav_count_profiles({ count: 0 })}`,
  ])

  for (const [name, count] of [
    ['Work', 2],
    ['Home', 1],
    ['Travel', 0],
  ] as const) {
    await expect
      .element(
        folderLink(name).getByText(m.profile_nav_count_profiles({ count }), {
          exact: true,
        }),
      )
      .toBeInTheDocument()
  }

  await expect
    .element(folderLink('Home'))
    .toHaveAttribute('href', '/main/profiles/folder/f2')
})

test('the open folder is the selected item', async () => {
  await renderAt('/main/profiles/folder/f2')

  await expect
    .element(folderLink('Home'))
    .toHaveAttribute('aria-current', 'page')
  await expect.element(folderLink('Work')).not.toHaveAttribute('aria-current')
  await expect
    .element(
      page.getByRole('link', {
        name: m.profile_nav_all_profiles(),
        exact: false,
      }),
    )
    .not.toHaveAttribute('aria-current')
})

test('the add button creates a folder', async () => {
  const { created } = await renderAt('/main/profiles/profile')

  await page.getByRole('button', { name: m.profile_folder_new_title() }).click()
  await userEvent.type(
    page
      .getByRole('textbox', { name: m.profile_folder_name_label() })
      .element(),
    'Games',
  )
  await page.getByRole('button', { name: m.common_save() }).click()

  await expect.poll(() => created).toEqual([['Games', []]])
})

test('a folder is renamed from its menu', async () => {
  const { layouts } = await renderAt('/main/profiles/profile')

  await openMenu('Home')
  await page
    .getByRole('menuitem', { name: m.profile_folder_rename_title() })
    .click()

  const input = page.getByRole('textbox', {
    name: m.profile_folder_name_label(),
  })
  await expect.element(input).toHaveValue('Home')
  await input.fill('House')
  await expect.element(input).toHaveValue('House')
  await page.getByRole('button', { name: m.common_save() }).click()

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].folders.map((folder) => folder.name)).toEqual([
    'Work',
    'House',
    'Travel',
  ])
  await expect.element(folderLink('House')).toBeInTheDocument()
})

test('deleting a folder with profiles asks first; an empty one does not', async () => {
  const { layouts } = await renderAt('/main/profiles/profile')
  vi.mocked(ask).mockResolvedValueOnce(false)

  await openMenu('Work')
  await page
    .getByRole('menuitem', { name: m.profile_folder_delete_title() })
    .click()
  await expect.poll(() => vi.mocked(ask).mock.calls).toHaveLength(1)
  expect(layouts).toHaveLength(0)
  await closeMenu()

  await openMenu('Travel')
  await page
    .getByRole('menuitem', { name: m.profile_folder_delete_title() })
    .click()

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].folders.map((folder) => folder.id)).toEqual(['f1', 'f2'])
  expect(vi.mocked(ask).mock.calls).toHaveLength(1)
})

test('Move up and Move down reorder the folders in one commit', async () => {
  const { layouts } = await renderAt('/main/profiles/profile')

  await openMenu('Home')
  await page.getByRole('menuitem', { name: m.profile_folder_move_up() }).click()

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].folders.map((folder) => folder.id)).toEqual([
    'f2',
    'f1',
    'f3',
  ])
  await closeMenu()

  await openMenu('Work')
  await page
    .getByRole('menuitem', { name: m.profile_folder_move_down() })
    .click()

  await expect.poll(() => layouts).toHaveLength(2)
  expect(layouts[1].folders.map((folder) => folder.id)).toEqual([
    'f2',
    'f3',
    'f1',
  ])
})

test('the first folder cannot move up and the last cannot move down', async () => {
  await renderAt('/main/profiles/profile')

  await openMenu('Work')
  await expect
    .element(page.getByRole('menuitem', { name: m.profile_folder_move_up() }))
    .toHaveAttribute('aria-disabled', 'true')
  await userEvent.keyboard('{Escape}')
  await closeMenu()

  await openMenu('Travel')
  await expect
    .element(page.getByRole('menuitem', { name: m.profile_folder_move_down() }))
    .toHaveAttribute('aria-disabled', 'true')
})

test('dragging a folder over another reorders the folders in one commit', async () => {
  const { layouts } = await renderAt('/main/profiles/profile')

  await drag(folderLink('Home').element(), folderLink('Work').element())

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].folders.map((folder) => folder.id)).toEqual([
    'f2',
    'f1',
    'f3',
  ])
  expect(layouts[0].folders.map((folder) => folder.members)).toEqual([
    ['c'],
    ['a', 'b'],
    [],
  ])
})

test('a folder moves with the keyboard', async () => {
  const { layouts } = await renderAt('/main/profiles/profile')

  folderLink('Work').element().focus()
  await userEvent.keyboard(' ')
  await userEvent.keyboard('{ArrowDown}')
  await userEvent.keyboard(' ')

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].folders.map((folder) => folder.id)).toEqual([
    'f2',
    'f1',
    'f3',
  ])
})

test('a click on a folder still opens it', async () => {
  const { layouts } = await renderAt('/main/profiles/profile')

  await folderLink('Home').click()

  await expect
    .element(folderLink('Home'))
    .toHaveAttribute('aria-current', 'page')
  expect(layouts).toEqual([])
})
