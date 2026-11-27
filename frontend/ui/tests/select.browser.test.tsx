import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test } from 'vitest'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@nyanpasu/ui/select'

function mount(
  element: React.ReactNode,
  onTestFinished: (fn: () => void) => void,
) {
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
  })
  root.render(element)
  return container
}

test('Select displays its selected item label while closed before opening and after closing', async ({
  onTestFinished,
}) => {
  function Demo() {
    const [value, setValue] = useState('mihomo')

    return (
      <Select variant="outlined" value={value} onValueChange={setValue}>
        <SelectTrigger aria-label="Proxy core">
          <SelectValue placeholder="Select proxy core" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="mihomo">Mihomo (Meta)</SelectItem>
          <SelectItem value="clash-rs">Clash RS</SelectItem>
        </SelectContent>
      </Select>
    )
  }

  const container = mount(<Demo />, onTestFinished)

  await expect
    .poll(() => container.querySelector('button[role="combobox"]')?.textContent)
    .toContain('Mihomo (Meta)')
})
