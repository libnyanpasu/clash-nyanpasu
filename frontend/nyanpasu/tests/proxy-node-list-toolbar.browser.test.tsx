import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { type NodeView } from '@/pages/(main)/main/proxies/group/_modules/node-list'
import {
  HideUnavailableButton,
  LocateCurrentNodeButton,
  NodeSearchOverlay,
  NoMatchingNodes,
  SearchNodesButton,
  SortNodesButton,
} from '@/pages/(main)/main/proxies/group/_modules/node-list-toolbar'
import { m } from '@/paraglide/messages'

async function renderSearch({ initial = '' }: { initial?: string } = {}) {
  const onSearchChange = vi.fn()
  function Harness() {
    const [search, setSearch] = useState(initial)
    const [open, setOpen] = useState(false)
    return (
      <SearchNodesButton
        open={open}
        onOpenChange={setOpen}
        search={search}
        onSearchChange={(next) => {
          onSearchChange(next)
          setSearch(next)
        }}
      />
    )
  }
  const screen = await render(
    <TooltipProvider>
      <Harness />
    </TooltipProvider>,
  )
  return { screen, onSearchChange }
}

test('searching opens from an icon and the clear button empties it', async () => {
  const { screen, onSearchChange } = await renderSearch()
  expect(
    screen.container.querySelector('[data-slot="proxies-node-search-field"]'),
  ).toBeNull()

  await screen
    .getByRole('button', { name: m.proxies_node_search_placeholder() })
    .click()

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
  await expect.element(searchbox).toHaveValue('')
})

test('an open search shows the field instead of the icon', async () => {
  const screen = await render(
    <TooltipProvider>
      <SearchNodesButton
        open
        search=""
        onSearchChange={() => {}}
        onOpenChange={() => {}}
      />
    </TooltipProvider>,
  )

  await expect.element(screen.getByRole('searchbox')).toBeVisible()
  expect(
    screen.container.querySelector('[data-slot="proxies-node-search-button"]'),
  ).toBeNull()
})

test('the overlay search closes from its close button', async () => {
  const onClose = vi.fn()
  const screen = await render(
    <TooltipProvider>
      <NodeSearchOverlay
        search=""
        onSearchChange={() => {}}
        onClose={onClose}
      />
    </TooltipProvider>,
  )

  await expect.element(screen.getByRole('searchbox')).toBeVisible()
  await screen.getByRole('button', { name: m.common_close() }).click()
  expect(onClose).toHaveBeenCalled()
})

test('the sort menu checks the current sort and picks another', async () => {
  const view: NodeView = { sort: 'name', hideUnavailable: true }
  const onViewChange = vi.fn()
  const screen = await render(
    <TooltipProvider>
      <SortNodesButton view={view} onViewChange={onViewChange} />
    </TooltipProvider>,
  )

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

test('the hide-unavailable button hides unavailable nodes when pressed', async () => {
  const onPressedChange = vi.fn()
  const screen = await render(
    <TooltipProvider>
      <HideUnavailableButton
        pressed={false}
        onPressedChange={onPressedChange}
      />
    </TooltipProvider>,
  )
  const button = screen.getByRole('button', {
    name: m.proxies_node_hide_unavailable(),
  })
  await expect.element(button).toHaveAttribute('aria-pressed', 'false')
  await button.click()
  expect(onPressedChange).toHaveBeenCalledWith(true)
})

test('a pressed hide-unavailable button shows every node again', async () => {
  const onPressedChange = vi.fn()
  const screen = await render(
    <TooltipProvider>
      <HideUnavailableButton pressed onPressedChange={onPressedChange} />
    </TooltipProvider>,
  )
  const button = screen.getByRole('button', {
    name: m.proxies_node_hide_unavailable(),
  })
  await expect.element(button).toHaveAttribute('aria-pressed', 'true')
  await button.click()
  expect(onPressedChange).toHaveBeenCalledWith(false)
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
