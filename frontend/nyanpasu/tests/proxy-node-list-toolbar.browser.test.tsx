import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import {
  DEFAULT_NODE_VIEW,
  type NodeView,
} from '@/pages/(main)/main/proxies/group/_modules/node-list'
import {
  LocateCurrentNodeButton,
  NodeListToolbar,
  NoMatchingNodes,
} from '@/pages/(main)/main/proxies/group/_modules/node-list-toolbar'
import { m } from '@/paraglide/messages'

async function renderToolbar({
  search = '',
  view = DEFAULT_NODE_VIEW,
}: { search?: string; view?: NodeView } = {}) {
  const onSearchChange = vi.fn()
  const onViewChange = vi.fn()
  const screen = await render(
    <TooltipProvider>
      <NodeListToolbar
        search={search}
        onSearchChange={onSearchChange}
        view={view}
        onViewChange={onViewChange}
      />
    </TooltipProvider>,
  )
  return { screen, onSearchChange, onViewChange }
}

test('the toolbar root is named by its slot', async () => {
  const { screen } = await renderToolbar()
  expect(
    screen.container.querySelector('[data-slot="proxies-node-list-toolbar"]'),
  ).not.toBeNull()
})

test('typing searches and the clear button empties the search', async () => {
  const { screen, onSearchChange } = await renderToolbar({ search: 'hk' })
  const searchbox = screen.getByRole('searchbox')
  await expect
    .element(searchbox)
    .toHaveAttribute('placeholder', m.proxies_node_search_placeholder())
  await searchbox.fill('hk 01')
  expect(onSearchChange).toHaveBeenLastCalledWith('hk 01')

  await screen
    .getByRole('button', { name: m.proxies_node_search_clear() })
    .click()
  expect(onSearchChange).toHaveBeenLastCalledWith('')
})

test('the sort menu checks the current sort and picks another', async () => {
  const view: NodeView = { sort: 'name', hideUnavailable: true }
  const { screen, onViewChange } = await renderToolbar({ view })

  await screen
    .getByRole('button', { name: m.proxies_node_sort_label() })
    .click()

  const items = screen.getByRole('menuitemradio')
  await expect.poll(() => items.elements().length).toBe(3)
  await expect
    .element(
      screen.getByRole('menuitemradio', { name: m.proxies_node_sort_name() }),
    )
    .toHaveAttribute('aria-checked', 'true')
  await expect
    .element(
      screen.getByRole('menuitemradio', {
        name: m.proxies_node_sort_default(),
      }),
    )
    .toHaveAttribute('aria-checked', 'false')

  await screen
    .getByRole('menuitemradio', { name: m.proxies_node_sort_delay() })
    .click()
  expect(onViewChange).toHaveBeenCalledWith({ ...view, sort: 'delay' })
})

test('the hide-unavailable chip hides unavailable nodes when pressed', async () => {
  const off = await renderToolbar()
  const chip = off.screen.getByRole('button', {
    name: m.proxies_node_hide_unavailable(),
  })
  await expect.element(chip).toHaveAttribute('aria-pressed', 'false')
  await chip.click()
  expect(off.onViewChange).toHaveBeenCalledWith({
    ...DEFAULT_NODE_VIEW,
    hideUnavailable: true,
  })
})

test('a pressed hide-unavailable chip shows every node again', async () => {
  const on = await renderToolbar({
    view: { ...DEFAULT_NODE_VIEW, hideUnavailable: true },
  })
  const pressed = on.screen.getByRole('button', {
    name: m.proxies_node_hide_unavailable(),
  })
  await expect.element(pressed).toHaveAttribute('aria-pressed', 'true')
  await pressed.click()
  expect(on.onViewChange).toHaveBeenCalledWith(DEFAULT_NODE_VIEW)
})

test('locating is disabled while the current node is not listed', async () => {
  const onLocate = vi.fn()
  const screen = await render(
    <TooltipProvider>
      <LocateCurrentNodeButton disabled onLocate={onLocate} />
    </TooltipProvider>,
  )
  const button = screen.getByRole('button', {
    name: m.proxies_locate_current_node(),
  })
  await expect.element(button).toBeDisabled()
  button.element().dispatchEvent(new MouseEvent('click', { bubbles: true }))
  expect(onLocate).not.toHaveBeenCalled()
})

test('locating scrolls once per click', async () => {
  const onLocate = vi.fn()
  const screen = await render(
    <TooltipProvider>
      <LocateCurrentNodeButton disabled={false} onLocate={onLocate} />
    </TooltipProvider>,
  )
  await screen
    .getByRole('button', { name: m.proxies_locate_current_node() })
    .click()
  expect(onLocate).toHaveBeenCalledTimes(1)
})

test('an empty match says so', async () => {
  const screen = await render(<NoMatchingNodes />)
  await expect
    .element(screen.getByText(m.proxies_node_no_match()))
    .toBeVisible()
  expect(
    screen.container.querySelector('[data-slot="proxies-node-no-match"]'),
  ).not.toBeNull()
})
