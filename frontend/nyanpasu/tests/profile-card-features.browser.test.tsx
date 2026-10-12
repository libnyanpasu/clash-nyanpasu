import { expect, test } from 'vitest'
import { render } from 'vitest-browser-react'
import { page, userEvent } from 'vitest/browser'
import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import ProfileCardFeatures from '@/pages/(main)/main/profiles/$type/_modules/profile-card-features'
import { m } from '@/paraglide/messages'
import { getLocale, overwriteGetLocale } from '@/paraglide/runtime'
import type { ProfileItem_Serialize } from '@nyanpasu/rpc/types'

const transforms: ProfileItem_Serialize[] = [
  {
    uid: 'script',
    name: 'Node ordering',
    type: 'transform',
    transform: {
      type: 'script',
      runtime: 'javascript',
      source: {
        type: 'local',
        binding: { type: 'managed', file: 'script.js' },
      },
    },
  },
  {
    uid: 'overlay',
    name: 'Rule overrides',
    type: 'transform',
    transform: {
      type: 'overlay',
      source: {
        type: 'local',
        binding: { type: 'managed', file: 'overlay.yaml' },
      },
    },
  },
]

test.for(['en', 'zh-cn'] as const)(
  'summary chips in %s expose only scoped transforms in execution order',
  async (locale, { onTestFinished }) => {
    const previousGetLocale = getLocale
    overwriteGetLocale(() => locale)
    const profile: ProfileItem_Serialize = {
      uid: 'subscription',
      name: 'Subscription',
      type: 'config',
      config: {
        type: 'file',
        source: {
          type: 'remote',
          file: 'subscription.yaml',
          url: 'https://example.com/sub',
          option: {
            with_proxy: true,
            self_proxy: true,
            update_interval_minutes: 120,
          },
        },
        transforms: ['overlay', 'script'],
      },
    }
    const view = await render(
      <TooltipProvider>
        <div className="bg-surface text-on-surface w-full p-4">
          <ProfileCardFeatures
            profile={profile}
            profiles={transforms}
            isGlobal={false}
          />
        </div>
      </TooltipProvider>,
    )
    onTestFinished(async () => {
      await view.unmount()
      overwriteGetLocale(previousGetLocale)
    })

    await expect
      .element(view.getByText(m.profile_source_remote()))
      .toBeVisible()
    await expect
      .element(view.getByText(m.profile_card_auto_update({ interval: '2h' })))
      .toBeVisible()
    const trigger = view.getByRole('button', {
      name: m.profile_card_transform_count({ count: 2 }),
    })
    await userEvent.hover(trigger)
    await expect
      .element(page.getByText(m.profile_card_scoped_transforms_title()))
      .toBeVisible()
    const details = document.querySelector(
      '[data-slot="profile-card-transform-details"]',
    )!
    expect(
      Array.from(details.querySelectorAll('li')).map((row) => row.textContent),
    ).toEqual(['1.Rule overridesYAML', '2.Node orderingJavaScript'])
    await userEvent.keyboard('{Escape}')
    await expect
      .poll(() =>
        document.querySelector('[data-slot="profile-card-transform-details"]'),
      )
      .toBeNull()
    await userEvent.unhover(trigger)
    trigger.element().focus()
    await expect
      .element(page.getByText(m.profile_card_scoped_transforms_title()))
      .toBeVisible()
    await userEvent.keyboard('{Escape}')

    for (const width of [390, 320]) {
      await page.viewport(width, 700)
      const row = document.querySelector('[data-slot="profile-card-type"]')!
      expect(row.scrollWidth).toBeLessThanOrEqual(row.clientWidth)
    }
  },
)

test('composition counts distinct config sources and hides empty transforms', async ({
  onTestFinished,
}) => {
  const profile: ProfileItem_Serialize = {
    uid: 'composition',
    name: 'Composition',
    type: 'config',
    config: {
      type: 'composition',
      base: 'base',
      extend_proxies_from: ['first', 'second'],
    },
  }
  const view = await render(
    <ProfileCardFeatures profile={profile} profiles={[]} isGlobal={false} />,
  )
  onTestFinished(() => view.unmount())
  await expect
    .element(view.getByText(m.profile_card_source_count({ count: 3 })))
    .toBeVisible()
  expect(
    document
      .querySelector('[data-slot="profile-card-type"]')
      ?.querySelector('button'),
  ).toBeNull()

  await view.rerender(
    <ProfileCardFeatures
      profile={{
        ...profile,
        config: {
          type: 'composition',
          extend_proxies_from: ['first', 'first'],
        },
      }}
      profiles={[]}
      isGlobal={false}
    />,
  )
  await expect
    .element(view.getByText(m.profile_card_source_count({ count: 1 })))
    .toBeVisible()
})

test('transform chips show runtime and global membership without config enhancements', async ({
  onTestFinished,
}) => {
  const view = await render(
    <ProfileCardFeatures
      profile={transforms[0]}
      profiles={transforms}
      isGlobal
    />,
  )
  onTestFinished(() => view.unmount())
  await expect.element(view.getByText('JavaScript')).toBeVisible()
  await expect.element(view.getByText(m.profile_global_label())).toBeVisible()
  expect(
    document
      .querySelector('[data-slot="profile-card-type"]')
      ?.querySelector('button'),
  ).toBeNull()
})
