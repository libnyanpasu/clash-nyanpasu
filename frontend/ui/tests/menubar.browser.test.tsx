import '../../theme/src/styles/theme.css'
import '@/assets/styles/tailwind.css'
import { createRef, useState } from 'react'
import { expect, onTestFinished, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page, userEvent } from 'vitest/browser'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import {
  Menubar,
  MenubarCheckboxItem,
  MenubarContent,
  MenubarItem,
  MenubarMenu,
  MenubarRadioGroup,
  MenubarRadioItem,
  MenubarSub,
  MenubarSubContent,
  MenubarSubTrigger,
  MenubarTrigger,
} from '@nyanpasu/ui/menubar'

test('top-level arrow keys skip disabled triggers and Escape restores focus', async () => {
  await render(
    <Menubar>
      <MenubarMenu>
        <MenubarTrigger>File</MenubarTrigger>
        <MenubarContent>
          <MenubarItem>New</MenubarItem>
        </MenubarContent>
      </MenubarMenu>

      <MenubarMenu>
        <MenubarTrigger disabled>Edit</MenubarTrigger>
        <MenubarContent>
          <MenubarItem>Undo</MenubarItem>
        </MenubarContent>
      </MenubarMenu>

      <MenubarMenu>
        <MenubarTrigger>View</MenubarTrigger>
        <MenubarContent>
          <MenubarItem>Zoom</MenubarItem>
        </MenubarContent>
      </MenubarMenu>
    </Menubar>,
  )

  const file = page.getByRole('menuitem', { name: 'File' })
  const view = page.getByRole('menuitem', { name: 'View' })

  await file.click()
  await userEvent.keyboard('{ArrowRight}')
  await expect.element(view).toHaveAttribute('aria-expanded', 'true')
  await expect
    .element(page.getByRole('menuitem', { name: 'Zoom' }))
    .toBeVisible()
  await userEvent.keyboard('{Escape}')
  await expect.element(view).toHaveFocus()
  await userEvent.keyboard('{ArrowLeft}')
  await expect.element(file).toHaveFocus()

  await userEvent.keyboard('{ArrowDown}')
  await expect
    .element(page.getByRole('menuitem', { name: 'New' }))
    .toBeVisible()
  await userEvent.keyboard('{Escape}')
  await expect.element(file).toHaveFocus()
})

test('controlled checkbox and radio items update through their callbacks', async () => {
  const onCheckboxSelect = vi.fn()
  const onRadioSelect = vi.fn()

  function Example() {
    const [checked, setChecked] = useState(false)
    const [value, setValue] = useState('compact')

    return (
      <Menubar>
        <MenubarMenu>
          <MenubarTrigger>View</MenubarTrigger>
          <MenubarContent>
            <MenubarCheckboxItem
              checked={checked}
              onCheckedChange={setChecked}
              onSelect={onCheckboxSelect}
            >
              Show sidebar
            </MenubarCheckboxItem>
            <MenubarRadioGroup value={value} onValueChange={setValue}>
              <MenubarRadioItem value="compact" onSelect={onRadioSelect}>
                Compact
              </MenubarRadioItem>
              <MenubarRadioItem value="comfortable" onSelect={onRadioSelect}>
                Comfortable
              </MenubarRadioItem>
            </MenubarRadioGroup>
          </MenubarContent>
        </MenubarMenu>
      </Menubar>
    )
  }

  await render(<Example />)

  const trigger = page.getByRole('menuitem', { name: 'View' })
  await trigger.click()
  const checkbox = page.getByRole('menuitemcheckbox', { name: 'Show sidebar' })
  await checkbox.click()
  await expect.element(checkbox).not.toBeInTheDocument()
  expect(onCheckboxSelect).toHaveBeenCalledOnce()

  await trigger.click()
  await expect
    .element(page.getByRole('menuitemcheckbox', { name: 'Show sidebar' }))
    .toHaveAttribute('aria-checked', 'true')

  await page.getByRole('menuitemradio', { name: 'Comfortable' }).click()
  expect(onRadioSelect).toHaveBeenCalledOnce()

  await trigger.click()
  await expect
    .element(page.getByRole('menuitemradio', { name: 'Comfortable' }))
    .toHaveAttribute('aria-checked', 'true')
  await expect
    .element(page.getByRole('menuitemradio', { name: 'Compact' }))
    .toHaveAttribute('aria-checked', 'false')
})

test('submenu opens with the keyboard and its content is portaled', async () => {
  const view = await render(
    <Menubar>
      <MenubarMenu>
        <MenubarTrigger>File</MenubarTrigger>
        <MenubarContent>
          <MenubarSub>
            <MenubarSubTrigger>Export</MenubarSubTrigger>
            <MenubarSubContent>
              <MenubarItem>As image</MenubarItem>
            </MenubarSubContent>
          </MenubarSub>
        </MenubarContent>
      </MenubarMenu>
    </Menubar>,
  )

  await page.getByRole('menuitem', { name: 'File' }).click()
  const exportTrigger = page.getByRole('menuitem', { name: 'Export' })
  exportTrigger.element().focus()
  await userEvent.keyboard('{ArrowRight}')

  const submenuItem = page.getByRole('menuitem', { name: 'As image' })
  await expect.element(submenuItem).toBeVisible()
  expect(view.container.contains(submenuItem.element())).toBe(false)
  await userEvent.keyboard('{Enter}')
  await expect.element(submenuItem).not.toBeInTheDocument()
})

test('menu items keep their icon and label aligned to the start', async () => {
  await render(
    <Menubar>
      <MenubarMenu>
        <MenubarTrigger>File</MenubarTrigger>
        <MenubarContent>
          <MenubarItem>
            <svg className="size-4" aria-hidden />
            <span>Open project</span>
          </MenubarItem>
        </MenubarContent>
      </MenubarMenu>
    </Menubar>,
  )

  await page.getByRole('menuitem', { name: 'File' }).click()
  const item = page.getByRole('menuitem', { name: 'Open project' })
  await expect.element(item).toBeVisible()

  const icon = item.element().querySelector('svg')!
  const label = item.element().querySelector('span')!
  expect(label.getBoundingClientRect().left).toBeLessThan(
    icon.getBoundingClientRect().right + 16,
  )
  expect(getComputedStyle(item.element()).justifyContent).not.toBe(
    'space-between',
  )
})

test('checkbox asChild preserves its child, built-in indicator, and ref', async () => {
  const checkboxRef = createRef<HTMLButtonElement>()
  const onCheckedChange = vi.fn()

  await render(
    <Menubar>
      <MenubarMenu>
        <MenubarTrigger>View</MenubarTrigger>
        <MenubarContent>
          <MenubarCheckboxItem
            asChild
            checked
            onCheckedChange={onCheckedChange}
          >
            <button ref={checkboxRef} type="button">
              <svg data-testid="sidebar-icon" aria-hidden />
              <span>Show sidebar</span>
            </button>
          </MenubarCheckboxItem>
        </MenubarContent>
      </MenubarMenu>
    </Menubar>,
  )

  await page.getByRole('menuitem', { name: 'View' }).click()
  const checkbox = page.getByRole('menuitemcheckbox', { name: 'Show sidebar' })
  await expect.element(checkbox).toHaveAttribute('aria-checked', 'true')
  await expect
    .element(checkbox)
    .toHaveAttribute('data-slot', 'menubar-checkbox-item')
  expect(checkboxRef.current).toBe(checkbox.element())
  expect(
    checkboxRef.current?.querySelector('[data-testid="sidebar-icon"]'),
  ).not.toBeNull()
  expect(
    checkboxRef.current?.querySelector('[data-slot="menubar-item-indicator"]'),
  ).not.toBeNull()

  await checkbox.click()
  expect(onCheckedChange).toHaveBeenCalledWith(false)
})

test.each(['light', 'dark'])(
  'Menubar menu styling matches DropdownMenu in %s mode',
  async (mode) => {
    const originalClasses = document.documentElement.className
    document.documentElement.classList.toggle('dark', mode === 'dark')
    onTestFinished(() => {
      document.documentElement.className = originalClasses
    })

    const readStyles = (surface: Element, item: Element, selected: Element) => {
      const surfaceStyle = getComputedStyle(surface)
      const itemStyle = getComputedStyle(item)
      const selectedStyle = getComputedStyle(selected)
      return {
        surface: {
          radius: surfaceStyle.borderTopLeftRadius,
          background: surfaceStyle.backgroundColor,
          borderColor: surfaceStyle.borderTopColor,
          borderWidth: surfaceStyle.borderTopWidth,
        },
        item: {
          height: itemStyle.height,
          paddingLeft: itemStyle.paddingLeft,
          paddingRight: itemStyle.paddingRight,
        },
        selectedBackground: selectedStyle.backgroundColor,
      }
    }

    const dropdownView = await render(
      <DropdownMenu>
        <DropdownMenuTrigger>Options</DropdownMenuTrigger>
        <DropdownMenuContent>
          <DropdownMenuItem>Regular item</DropdownMenuItem>
          <DropdownMenuRadioGroup value="selected">
            <DropdownMenuRadioItem value="selected">
              Selected item
            </DropdownMenuRadioItem>
          </DropdownMenuRadioGroup>
        </DropdownMenuContent>
      </DropdownMenu>,
    )
    await page.getByRole('button', { name: 'Options' }).click()
    await expect.element(page.getByRole('menu')).toBeVisible()
    const dropdownSurface = page.getByRole('menu').element()
    const dropdownItem = page
      .getByRole('menuitem', { name: 'Regular item' })
      .element()
    const dropdownSelected = page
      .getByRole('menuitemradio', { name: 'Selected item' })
      .element()
    const dropdownStyles = readStyles(
      dropdownSurface,
      dropdownItem,
      dropdownSelected,
    )
    await dropdownView.unmount()

    const menubarView = await render(
      <Menubar>
        <MenubarMenu>
          <MenubarTrigger>Options</MenubarTrigger>
          <MenubarContent>
            <MenubarItem>Regular item</MenubarItem>
            <MenubarRadioGroup value="selected">
              <MenubarRadioItem value="selected">
                Selected item
              </MenubarRadioItem>
            </MenubarRadioGroup>
          </MenubarContent>
        </MenubarMenu>
      </Menubar>,
    )
    await page.getByRole('menuitem', { name: 'Options' }).click()
    await expect.element(page.getByRole('menu')).toBeVisible()
    const menubarSurface = page.getByRole('menu').element()
    const menubarItem = page
      .getByRole('menuitem', { name: 'Regular item' })
      .element()
    const menubarSelected = page
      .getByRole('menuitemradio', { name: 'Selected item' })
      .element()
    const menubarStyles = readStyles(
      menubarSurface,
      menubarItem,
      menubarSelected,
    )

    expect(menubarStyles).toEqual(dropdownStyles)
    await menubarView.unmount()
  },
)
