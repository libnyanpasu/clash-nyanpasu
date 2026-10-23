import '../../theme/src/styles/theme.css'
import '@/assets/styles/tailwind.css'
import { expect, test } from 'vitest'
import { render } from 'vitest-browser-react'
import { page } from 'vitest/browser'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'

test('menu items align their icon and label to the start', async () => {
  await render(
    <DropdownMenu defaultOpen>
      <DropdownMenuTrigger>Open</DropdownMenuTrigger>

      <DropdownMenuContent>
        <DropdownMenuItem>
          <svg className="size-4" aria-hidden />
          <span>Reset</span>
        </DropdownMenuItem>

        <DropdownMenuItem>
          <svg className="size-4" aria-hidden />
          <span>Expand all folders</span>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>,
  )

  const items = page.getByRole('menuitem')
  await expect.element(items.first()).toBeVisible()

  for (const item of items.elements()) {
    const label = item.querySelector('span')!
    const icon = item.querySelector('svg')!

    // The label follows the icon; it is not pushed to the far edge.
    expect(label.getBoundingClientRect().left).toBeLessThan(
      icon.getBoundingClientRect().right + 16,
    )
    expect(getComputedStyle(item).justifyContent).not.toBe('space-between')
  }
})
