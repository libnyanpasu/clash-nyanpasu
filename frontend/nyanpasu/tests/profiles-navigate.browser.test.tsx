import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { SnackbarProvider } from '@nyanpasu/ui/snackbar'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import { m } from '@/paraglide/messages'
import type { ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router'
import { mockIPC } from '@tauri-apps/api/mocks'
import ProfilesLayoutProvider from '../src/pages/(main)/main/profiles/_modules/profiles-layout-provider'
import ProfilesNavigate from '../src/pages/(main)/main/profiles/_modules/profiles-navigate'
import { TestQueryProvider } from './query-provider'

vi.hoisted(() => {
  vi.stubGlobal('isTauri', true)
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

const script = (
  uid: string,
  runtime: 'javascript' | 'lua',
): ProfileItem_Serialize => ({
  type: 'transform',
  uid,
  name: uid,
  transform: {
    type: 'script',
    runtime,
    source: { type: 'local', binding: { type: 'managed', file: `${uid}.js` } },
  },
})

const overlay = (uid: string): ProfileItem_Serialize => ({
  type: 'transform',
  uid,
  name: uid,
  transform: {
    type: 'overlay',
    source: {
      type: 'local',
      binding: { type: 'managed', file: `${uid}.yaml` },
    },
  },
})

const items = [
  config('work'),
  config('home'),
  script('rewrite', 'javascript'),
  script('rename', 'javascript'),
  script('filter', 'javascript'),
  script('probe', 'lua'),
  overlay('dns'),
  overlay('tun'),
]

const renderAt = async (path: string) => {
  mockIPC((command, args) => {
    const method =
      command === 'call_rpc' ? (args as { method: string }).method : command

    if (method === 'get_profiles') {
      return {
        current: 'work',
        items,
        global_transforms: ['rewrite', 'dns'],
        valid: [],
      }
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
              <ProfilesLayoutProvider>
                <ProfilesNavigate />
              </ProfilesLayoutProvider>
            </SnackbarProvider>
          </TooltipProvider>
        </BlockTaskProvider>
      </TestQueryProvider>
    ),
  })
  const pageRoute = createRoute({
    getParentRoute: () => rootRoute,
    path,
  })
  const router = createRouter({
    routeTree: rootRoute.addChildren([pageRoute]),
    history: createMemoryHistory({ initialEntries: [path] }),
  })

  return render(<RouterProvider router={router} />)
}

const item = (
  view: Awaited<ReturnType<typeof renderAt>>,
  label: string | RegExp,
) => view.getByRole('link', { name: label, exact: false })

test('the profiles navigation groups its items under headings', async () => {
  const view = await renderAt('/main/profiles/profile')

  for (const heading of [
    m.profile_group_config(),
    m.profile_group_transform(),
    m.profile_group_pipeline(),
  ]) {
    await expect
      .element(view.getByRole('heading', { name: heading, exact: true }))
      .toBeInTheDocument()
  }

  await expect
    .element(view.getByRole('group', { name: m.profile_group_pipeline() }))
    .toBeInTheDocument()
})

test('each item counts its own kind of profile', async () => {
  const view = await renderAt('/main/profiles/profile')

  const supporting = [
    [m.profile_nav_all_profiles(), m.profile_nav_count_profiles({ count: 2 })],
    [m.profile_nav_javascript(), m.profile_nav_count_scripts({ count: 3 })],
    // One script takes the singular, which the plural messages distinguish.
    [m.profile_nav_lua(), m.profile_nav_count_scripts({ count: 1 })],
    [m.profile_nav_merge(), m.profile_nav_count_overlays({ count: 2 })],
    [
      m.profile_global_transform_title(),
      m.profile_nav_count_enabled({ count: 2 }),
    ],
    [m.inspect_title(), m.profile_nav_inspect_description()],
  ] as const

  for (const [label, description] of supporting) {
    await expect
      .element(item(view, label).getByText(description, { exact: true }))
      .toBeInTheDocument()
  }
})

test('an item stays selected on a detail page under its type', async () => {
  const view = await renderAt('/main/profiles/javascript/detail/rewrite')

  const javascript = item(view, m.profile_nav_javascript())
  await expect.element(javascript).toHaveAttribute('data-active', 'true')
  await expect.element(javascript).toHaveAttribute('aria-current', 'page')

  for (const label of [
    m.profile_nav_all_profiles(),
    m.profile_nav_lua(),
    m.profile_nav_merge(),
    m.profile_global_transform_title(),
    m.inspect_title(),
  ]) {
    const other = item(view, label)
    await expect.element(other).toHaveAttribute('data-active', 'false')
    await expect.element(other).not.toHaveAttribute('aria-current')
  }
})

test('the pipeline items are selected on their own pages', async () => {
  const global = await renderAt('/main/profiles/global')

  await expect
    .element(item(global, m.profile_global_transform_title()))
    .toHaveAttribute('aria-current', 'page')
})
