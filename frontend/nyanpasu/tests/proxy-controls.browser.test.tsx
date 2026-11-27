import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test } from 'vitest'
import { page, userEvent } from 'vitest/browser'
import { ExpressiveChoice } from '@nyanpasu/ui/expressive-choice'
import { ShapeToggle } from '@nyanpasu/ui/shape-toggle'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'

const options = ['Rule', 'Global', 'Direct'].map((value) => ({
  value,
  label: value,
  description: `Traffic routing: ${value}`,
  icon: (
    <svg viewBox="0 0 24 24" aria-hidden>
      <path fill="currentColor" d="M12 2 22 12 12 22 2 12Z" />
    </svg>
  ),
}))

function FocusChoice() {
  const [value, setValue] = useState('Rule')
  return (
    <TooltipProvider>
      <ExpressiveChoice
        options={options}
        value={value}
        layout="focus"
        label="Proxy mode"
        onChange={setValue}
      />
    </TooltipProvider>
  )
}

test('keyboard focus survives a focus-card selection and pending controls cannot mutate', async ({
  onTestFinished,
}) => {
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
  })
  root.render(<FocusChoice />)
  const direct = page.getByRole('button', { name: 'Direct' })
  await expect.element(direct).toBeVisible()
  direct.element().focus()
  await userEvent.keyboard('{Enter}')
  await expect
    .element(page.getByRole('button', { name: 'Direct' }))
    .toHaveAttribute('aria-pressed', 'true')
  await expect
    .poll(() => document.activeElement?.getAttribute('data-slot'))
    .toBe('expressive-choice-current')

  let changes = 0
  root.render(
    <TooltipProvider>
      <div>
        <ExpressiveChoice
          options={options}
          value="Rule"
          layout="flex"
          label="Proxy mode"
          disabled
          onChange={() => changes++}
        />
        <ShapeToggle
          active
          icon={<span>◈</span>}
          label="Pending proxy"
          loading
          onClick={() => changes++}
        />
      </div>
    </TooltipProvider>,
  )
  await expect
    .element(page.getByRole('switch', { name: 'Pending proxy' }))
    .toBeDisabled()
  await expect
    .element(page.getByRole('switch', { name: 'Pending proxy' }))
    .toHaveAttribute('aria-busy', 'true')
  await expect
    .element(page.getByRole('button', { name: 'Global' }))
    .toBeDisabled()
  for (const element of [
    page.getByRole('button', { name: 'Global' }).element(),
    page.getByRole('switch', { name: 'Pending proxy' }).element(),
  ]) {
    if (element instanceof HTMLButtonElement) element.click()
  }
  expect(changes).toBe(0)
})
