import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import { SubscriptionCard } from '@/pages/(main)/main/profiles/$type/detail/_modules/subscription-card'
import { m } from '@/paraglide/messages'
import type {
  ProfileItem_Serialize,
  ProfileSubscriptionInfo,
} from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { TestQueryProvider } from './query-provider'

vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({ isMinimized: async () => false }),
}))

test.for<{
  name: string
  subscription?: ProfileSubscriptionInfo
  percent?: number
  remaining?: string
}>([
  {
    name: 'partly used',
    subscription: { upload: 10, download: 10, total: 100 },
    percent: 80,
    remaining: '80 B',
  },
  {
    name: 'unused',
    subscription: { upload: 0, download: 0, total: 100 },
    percent: 100,
    remaining: '100 B',
  },
  {
    name: 'exceeded',
    subscription: { upload: 100, download: 20, total: 100 },
    percent: 0,
    remaining: '0 B',
  },
  { name: 'missing' },
  { name: 'incomplete', subscription: { upload: 10, total: 100 } },
  { name: 'zero total', subscription: { upload: 0, download: 0, total: 0 } },
])(
  'details show remaining quota for $name subscriptions',
  async ({ subscription, percent, remaining }, { onTestFinished }) => {
    vi.stubGlobal('isTauri', true)
    mockIPC((command, args) => {
      expect(command).toBe('call_rpc')
      expect((args as { method: string }).method).toBe(
        'get_profile_sync_status',
      )
      return {
        scheduled: false,
        next_run_at: null,
        active: [],
        journal_degraded: false,
        registration_error: null,
        history_limit: 10,
      }
    })
    const queries = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    })
    const profile: ProfileItem_Serialize = {
      uid: 'subscription',
      name: 'Subscription',
      type: 'config',
      config: {
        type: 'file',
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
    }
    const view = await render(
      <TestQueryProvider client={queries}>
        <BlockTaskProvider>
          <TooltipProvider>
            <SubscriptionCard profile={profile} />
          </TooltipProvider>
        </BlockTaskProvider>
      </TestQueryProvider>,
    )
    onTestFinished(async () => {
      await view.unmount()
      queries.clear()
      clearMocks()
      vi.unstubAllGlobals()
    })
    if (percent === undefined) {
      await expect
        .element(view.getByText(m.profile_card_quota_unknown()))
        .toBeVisible()
      await expect
        .element(view.getByRole('progressbar'))
        .not.toBeInTheDocument()
    } else {
      await expect
        .element(
          view.getByRole('progressbar', { name: m.profile_card_remaining() }),
        )
        .toHaveAttribute('aria-valuenow', String(percent))
      await expect
        .element(
          view.getByText(
            `${m.profile_card_remaining()} ${percent.toFixed(2)}%`,
          ),
        )
        .toBeVisible()
      await expect.element(view.getByText(`${remaining} /100 B`)).toBeVisible()
    }
  },
)
