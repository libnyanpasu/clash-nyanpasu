import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import type { CoreAgentSkill } from '@nyanpasu/rpc/types'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import { Route } from '../src/pages/(main)/main/settings/clash/ai/index'

const state = vi.hoisted(() => ({
  serviceMode: false,
  skills: {
    data: undefined as CoreAgentSkill[] | undefined,
    error: null as Error | null,
    isPending: false,
  },
}))

vi.mock('@nyanpasu/query', async (original) => ({
  ...(await original<typeof import('@nyanpasu/query')>()),
  useSetting: (key: string) => ({
    value: key === 'core' ? 'meow-alpha' : state.serviceMode,
  }),
  useCoreAgentSkills: () => state.skills,
}))
vi.mock('@/components/page-title', () => ({ PageTitle: () => null }))
vi.mock('@/utils', () => ({ formatError: String }))
vi.mock('@/utils/notification', () => ({ message: vi.fn() }))

const MEOW_CLI: CoreAgentSkill = {
  name: 'meow-cli',
  dir: '/home/alice/.local/share/clash-nyanpasu/cores/meow-alpha/skills/meow-cli',
  installed: true,
}

const renderPage = () => {
  const rootRoute = createRootRoute({ component: Outlet })
  const pageRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/',
    component: Route.options.component,
  })
  const router = createRouter({
    routeTree: rootRoute.addChildren([pageRoute]),
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })

  return render(<RouterProvider router={router} />)
}

test('the page lists the skills of the current core', async () => {
  state.serviceMode = false
  state.skills = { data: [MEOW_CLI], error: null, isPending: false }
  const view = await renderPage()

  await expect
    .element(view.getByText(m.settings_ai_skill_prompt({ path: MEOW_CLI.dir })))
    .toBeInTheDocument()
  expect(view.getByText(m.settings_ai_skill_service_mode()).query()).toBeNull()

  await view.unmount()
})

test('service mode warns that the skill cannot find the core', async () => {
  state.serviceMode = true
  state.skills = { data: [MEOW_CLI], error: null, isPending: false }
  const view = await renderPage()

  await expect
    .element(view.getByText(m.settings_ai_skill_service_mode()))
    .toBeInTheDocument()

  await view.unmount()
})

test('a core without skills and a failed query are both stated', async () => {
  state.serviceMode = false
  state.skills = { data: [], error: null, isPending: false }
  const empty = await renderPage()
  await expect
    .element(empty.getByText(m.settings_ai_skill_none()))
    .toBeInTheDocument()
  await empty.unmount()

  state.skills = { data: undefined, error: new Error('boom'), isPending: false }
  const failed = await renderPage()
  await expect
    .element(failed.getByRole('alert'))
    .toHaveTextContent(m.settings_ai_skill_failed({ error: 'Error: boom' }))
  await failed.unmount()
})
