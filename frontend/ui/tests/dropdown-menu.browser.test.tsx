import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page, userEvent } from 'vitest/browser'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuPillTrigger,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
} from '@nyanpasu/ui/dropdown-menu'

test('DropdownMenuPillTrigger opens a launcher menu with grouped items and nested submenus', async () => {
  const onImport = vi.fn()

  function Example() {
    const [systemProxy, setSystemProxy] = useState(false)
    const [mode, setMode] = useState('rule')

    return (
      <DropdownMenu variant="launcher" align="start">
        <DropdownMenuPillTrigger>
          <span>Clash Nyanpasu</span>
        </DropdownMenuPillTrigger>

        <DropdownMenuContent>
          <DropdownMenuGroup>
            <DropdownMenuItem onSelect={onImport}>
              Import Profile
            </DropdownMenuItem>
          </DropdownMenuGroup>

          <DropdownMenuGroup>
            <DropdownMenuSub>
              <DropdownMenuSubTrigger>Proxy Settings</DropdownMenuSubTrigger>
              <DropdownMenuSubContent>
                <DropdownMenuGroup>
                  <DropdownMenuCheckboxItem
                    checked={systemProxy}
                    onCheckedChange={setSystemProxy}
                  >
                    System Proxy
                  </DropdownMenuCheckboxItem>
                </DropdownMenuGroup>

                <DropdownMenuRadioGroup value={mode} onValueChange={setMode}>
                  <DropdownMenuRadioItem value="rule">
                    Rule
                  </DropdownMenuRadioItem>
                  <DropdownMenuRadioItem value="global">
                    Global
                  </DropdownMenuRadioItem>
                </DropdownMenuRadioGroup>
              </DropdownMenuSubContent>
            </DropdownMenuSub>
          </DropdownMenuGroup>
        </DropdownMenuContent>
      </DropdownMenu>
    )
  }

  const view = await render(<Example />)

  const trigger = page.getByRole('button', { name: 'Clash Nyanpasu' })
  const triggerEl = trigger.element()
  await expect
    .element(trigger)
    .toHaveAttribute('data-slot', 'dropdown-menu-pill-trigger')
  await expect.element(trigger).toHaveAttribute('aria-expanded', 'false')

  await trigger.click()
  expect(triggerEl.getAttribute('aria-expanded')).toBe('true')
  await expect
    .element(page.getByRole('menu', { name: 'Clash Nyanpasu' }))
    .toBeVisible()

  const importItem = page.getByRole('menuitem', { name: 'Import Profile' })
  await expect.element(importItem).toBeVisible()
  expect(view.container.contains(importItem.element())).toBe(false)

  const content = document.querySelector('[data-slot="dropdown-menu-content"]')
  expect(content).not.toBeNull()
  expect(content?.className).toContain('bg-transparent')

  const proxyTrigger = page.getByRole('menuitem', { name: 'Proxy Settings' })
  proxyTrigger.element().focus()
  await userEvent.keyboard('{ArrowRight}')

  const sysProxyCheckbox = page.getByRole('menuitemcheckbox', {
    name: 'System Proxy',
  })
  await expect.element(sysProxyCheckbox).toBeVisible()

  const subContent = document.querySelector(
    '[data-slot="dropdown-menu-sub-content"]',
  )
  expect(subContent?.className).toContain('bg-transparent')

  await sysProxyCheckbox.click()
  await expect.element(sysProxyCheckbox).not.toBeInTheDocument()

  await trigger.click()
  await page.getByRole('menuitem', { name: 'Proxy Settings' }).click()
  await expect
    .element(page.getByRole('menuitemcheckbox', { name: 'System Proxy' }))
    .toHaveAttribute('aria-checked', 'true')

  await page.getByRole('menuitemradio', { name: 'Global' }).click()
  await trigger.click()
  await page.getByRole('menuitem', { name: 'Proxy Settings' }).click()
  await expect
    .element(page.getByRole('menuitemradio', { name: 'Global' }))
    .toHaveAttribute('aria-checked', 'true')

  await userEvent.keyboard('{Escape}')
})
