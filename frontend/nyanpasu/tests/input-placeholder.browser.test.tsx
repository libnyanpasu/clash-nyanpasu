import { flushSync } from 'react-dom'
import { createRoot } from 'react-dom/client'
import { expect, test } from 'vitest'
import { Input } from '@nyanpasu/ui/input'
// The app loads these unlayered, ahead of Tailwind's layers.
import '@csstools/normalize.css/normalize.css'
import '@csstools/normalize.css/opinionated.css'
import '@/assets/styles/tailwind.css'

const TRANSPARENT = 'rgba(0, 0, 0, 0)'

test.for(['filled', 'outlined'] as const)(
  'a %s input shows its placeholder only once focus floats the label',
  async (variant, { onTestFinished }) => {
    const container = document.createElement('div')
    document.body.append(container)
    const root = createRoot(container)
    onTestFinished(() => {
      root.unmount()
      container.remove()
    })

    flushSync(() => {
      root.render(
        <Input
          variant={variant}
          label="Listen"
          value=""
          placeholder="127.0.0.1:1053"
          onChange={() => {}}
        />,
      )
    })

    const input = container.querySelector('input')!
    const label = container.querySelector('label')!
    const placeholderColor = () =>
      getComputedStyle(input, '::placeholder').color
    const restingTop = getComputedStyle(label).top

    expect(placeholderColor()).toBe(TRANSPARENT)

    input.focus()

    await expect.poll(() => getComputedStyle(label).top).not.toBe(restingTop)
    expect(placeholderColor()).not.toBe(TRANSPARENT)
  },
)
