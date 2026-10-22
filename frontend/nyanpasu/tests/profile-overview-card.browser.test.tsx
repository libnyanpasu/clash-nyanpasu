import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import ProfileOverviewCard from '@/pages/(main)/main/profiles/$type/detail/_modules/profile-overview-card'
import { m } from '@/paraglide/messages'
import type {
  ProfileItem_Serialize,
  ProfileSubscriptionInfo,
} from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router'
import { mockIPC } from '@tauri-apps/api/mocks'
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

const remoteProfile = (
  subscription?: ProfileSubscriptionInfo,
): ProfileItem_Serialize => ({
  uid: 'subscription',
  name: 'Subscription',
  type: 'config',
  config: {
    type: 'file',
    transforms: [],
    source: {
      type: 'remote',
      file: 'subscription.yaml',
      url: 'https://example.test/sub',
      option: {
        with_proxy: false,
        self_proxy: false,
        update_interval_minutes: 0,
      },
      subscription,
    },
  },
})

const mount = async (
  profile: ProfileItem_Serialize,
  current: string,
  onTestFinished: (fn: () => void) => void,
) => {
  vi.stubGlobal('isTauri', true)
  mockIPC((command, args) => {
    const wire = args as { method: string }
    const method = command === 'call_rpc' ? wire.method : command
    if (method === 'get_profiles')
      return { current, items: [profile], global_transforms: [], valid: [] }
    if (method === 'read_profile_file') return 'proxies: [{name: Node}]'
    throw new Error(`Unexpected RPC ${method}`)
  })
  const queries = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  const container = document.createElement('div')
  container.style.width = '640px'
  container.className = 'bg-mixed-background text-on-surface'
  document.body.append(container)
  const root = createRoot(container)
  const route = createRootRoute({
    component: () => (
      <TestQueryProvider client={queries}>
        <BlockTaskProvider>
          <TooltipProvider>
            <ProfileOverviewCard
              profile={profile}
              profiles={[profile]}
              isGlobal={false}
            />
          </TooltipProvider>
        </BlockTaskProvider>
      </TestQueryProvider>
    ),
  })
  const router = createRouter({
    routeTree: route,
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })
  root.render(<RouterProvider router={router} />)
  onTestFinished(() => {
    root.unmount()
    container.remove()
    queries.clear()
    vi.unstubAllGlobals()
  })
  return container
}

test.for<{
  name: string
  subscription?: ProfileSubscriptionInfo
  percent?: number
  remaining?: string
  total?: string
}>([
  {
    name: 'partly used',
    subscription: { upload: 10, download: 10, total: 100 },
    percent: 80,
    remaining: '80 B',
    total: '100 B',
  },
  {
    name: 'unused',
    subscription: { upload: 0, download: 0, total: 100 },
    percent: 100,
    remaining: '100 B',
    total: '100 B',
  },
  {
    name: 'exceeded',
    subscription: { upload: 100, download: 20, total: 100 },
    percent: 0,
    remaining: '0 B',
    total: '100 B',
  },
  { name: 'missing' },
  { name: 'incomplete', subscription: { upload: 10, total: 100 } },
  { name: 'zero total', subscription: { upload: 0, download: 0, total: 0 } },
])(
  'the overview card shows remaining quota for $name subscriptions',
  async ({ subscription, percent, remaining, total }, { onTestFinished }) => {
    const container = await mount(
      remoteProfile(subscription),
      'other',
      onTestFinished,
    )
    const card = () =>
      container.querySelector<HTMLElement>(
        '[data-slot="profile-detail-overview"]',
      )

    if (percent === undefined) {
      await expect
        .poll(() => card()?.textContent)
        .toContain(m.profile_card_quota_unknown())
      expect(card()?.querySelector('[role="meter"]')).toBeNull()
    } else {
      await expect
        .poll(
          () =>
            card()?.querySelector('[data-slot="profile-card-remaining"]')
              ?.textContent,
        )
        .toBe(remaining)
      expect(card()?.textContent).toContain(
        m.profile_card_total({ total: total! }),
      )
      expect(
        card()?.querySelector('[role="meter"]')?.getAttribute('aria-valuenow'),
      ).toBe(String(percent))
    }
  },
)

test('the overview card has no name row and marks the active profile', async ({
  onTestFinished,
}) => {
  const profile = remoteProfile({ upload: 0, download: 0, total: 100 })
  const container = await mount(profile, profile.uid, onTestFinished)
  const card = () =>
    container.querySelector<HTMLElement>(
      '[data-slot="profile-detail-overview"]',
    )

  await expect.poll(() => card()?.getAttribute('data-active')).toBe('true')
  expect(card()?.textContent).toContain(m.profile_is_active_label())
  expect(card()?.textContent).not.toContain(profile.name)
  expect(card()?.querySelector('h1, h2, h3')).toBeNull()
})
