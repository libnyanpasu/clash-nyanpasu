import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { m } from '@/paraglide/messages'
import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router'
import CoreAgentSkillCard from '../src/pages/(main)/main/settings/clash/ai/_modules/core-agent-skill-card'

const state = vi.hoisted(() => ({
  skill: {
    name: 'meow-cli',
    dir: '/home/alice/.local/share/clash-nyanpasu/cores/meow-alpha/skills/meow-cli',
    installed: true,
  },
  copied: [] as string[],
}))

vi.mock('@/utils/clipboard', () => ({
  writeClipboardText: async (text: string) => {
    state.copied.push(text)
  },
}))
vi.mock('@/utils/notification', () => ({ message: vi.fn() }))

// The card links to Core Manager, which needs a router.
const renderCard = () => {
  const router = createRouter({
    routeTree: createRootRoute({
      component: () => <CoreAgentSkillCard skill={state.skill} />,
    }),
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })

  return render(<RouterProvider router={router} />)
}

test('the copied prompt names the installed skill folder', async () => {
  state.skill.installed = true
  state.copied = []
  const view = await renderCard()

  const prompt = m.settings_ai_skill_prompt({ path: state.skill.dir })
  await expect.element(view.getByText(prompt)).toBeInTheDocument()

  await view.getByRole('button', { name: m.common_copy() }).click()
  await expect.poll(() => state.copied).toEqual([prompt])
  expect(prompt).toContain(state.skill.dir)

  await view.unmount()
})

test('an uninstalled skill points to Core Manager', async () => {
  state.skill.installed = false
  const view = await renderCard()

  await expect
    .element(view.getByText(m.settings_ai_skill_not_installed()))
    .toBeInTheDocument()
  await expect
    .element(view.getByRole('link', { name: m.settings_label_core_manager() }))
    .toHaveAttribute('href', '/main/settings/clash/core-manager')
  expect(view.getByRole('button', { name: m.common_copy() }).query()).toBeNull()

  await view.unmount()
})
