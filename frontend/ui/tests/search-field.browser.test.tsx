import { ComponentProps, createRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { page, userEvent } from 'vitest/browser'
import { SearchField } from '@nyanpasu/ui/search-field'

function mount(
  onTestFinished: (fn: () => void) => void,
  {
    initialValue = '',
    onValueChange,
    fieldProps,
  }: {
    initialValue?: string
    onValueChange?: (value: string) => void
    fieldProps?: Partial<ComponentProps<typeof SearchField>>
  } = {},
) {
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  function Harness() {
    const [value, setValue] = useState(initialValue)
    return (
      <SearchField
        placeholder="Search nodes"
        clearLabel="Clear search"
        value={value}
        onValueChange={(next) => {
          onValueChange?.(next)
          setValue(next)
        }}
        {...fieldProps}
      />
    )
  }
  root.render(<Harness />)
  onTestFinished(() => {
    root.unmount()
    container.remove()
  })
  return container
}

test('renders a searchbox that reports every keystroke', async ({
  onTestFinished,
}) => {
  const onValueChange = vi.fn()
  mount(onTestFinished, { onValueChange })
  const input = page.getByRole('searchbox')
  await expect.element(input).toHaveAttribute('type', 'search')
  await expect.element(input).toHaveAttribute('placeholder', 'Search nodes')

  await userEvent.type(input, 'hk')
  await expect.element(input).toHaveValue('hk')
  expect(onValueChange.mock.calls).toEqual([['h'], ['hk']])
})

test('clear button appears with a value and clears from the keyboard', async ({
  onTestFinished,
}) => {
  mount(onTestFinished)
  const input = page.getByRole('searchbox')
  const clear = page.getByRole('button', { name: 'Clear search' })
  await expect.element(clear).not.toBeInTheDocument()

  await userEvent.type(input, 'jp')
  await expect.element(clear).toBeInTheDocument()

  await userEvent.tab()
  await expect.element(clear).toHaveFocus()
  await userEvent.keyboard('{Enter}')
  await expect.element(input).toHaveValue('')
  await expect.element(input).toHaveFocus()
  await expect.element(clear).not.toBeInTheDocument()
})

test('Escape clears a non-empty field', async ({ onTestFinished }) => {
  mount(onTestFinished, { initialValue: 'us' })
  const input = page.getByRole('searchbox')
  await userEvent.click(input)
  await userEvent.keyboard('{Escape}')
  await expect.element(input).toHaveValue('')
})

test('names the root, input and clear button with data-slot', async ({
  onTestFinished,
}) => {
  const container = mount(onTestFinished, { initialValue: 'sg' })
  await expect.element(page.getByRole('searchbox')).toBeInTheDocument()
  const root = container.querySelector('[data-slot="search-field"]')
  expect(root).not.toBeNull()
  expect(
    root!.querySelector('input[data-slot="search-field-input"]'),
  ).not.toBeNull()
  expect(
    root!.querySelector('button[data-slot="search-field-clear"]'),
  ).not.toBeNull()
})

async function inputOf(container: HTMLElement) {
  await expect.poll(() => container.querySelector('input')).not.toBeNull()
  return container.querySelector('input')!
}

test('names the input after the placeholder unless the caller names it', async ({
  onTestFinished,
}) => {
  const byDefault = await inputOf(mount(onTestFinished))
  expect(byDefault.getAttribute('aria-label')).toBe('Search nodes')

  const named = await inputOf(
    mount(onTestFinished, { fieldProps: { 'aria-label': 'Filter nodes' } }),
  )
  expect(named.getAttribute('aria-label')).toBe('Filter nodes')

  const labelled = await inputOf(
    mount(onTestFinished, { fieldProps: { 'aria-labelledby': 'external' } }),
  )
  expect(labelled.getAttribute('aria-label')).toBeNull()
  expect(labelled.getAttribute('aria-labelledby')).toBe('external')
})

test('a consumer ref receives the input while the clear button still focuses it', async ({
  onTestFinished,
}) => {
  const ref = createRef<HTMLInputElement>()
  const input = await inputOf(
    mount(onTestFinished, { initialValue: 'hk', fieldProps: { ref } }),
  )
  expect(ref.current).toBe(input)

  await userEvent.click(page.getByRole('button', { name: 'Clear search' }))
  expect(document.activeElement).toBe(input)
})

test('hides the clear button when the field is disabled or read-only', async ({
  onTestFinished,
}) => {
  for (const fieldProps of [{ disabled: true }, { readOnly: true }]) {
    const container = mount(onTestFinished, { initialValue: 'us', fieldProps })
    const input = await inputOf(container)
    expect(input.value).toBe('us')
    expect(
      container.querySelector('[data-slot="search-field-clear"]'),
    ).toBeNull()
  }
})
