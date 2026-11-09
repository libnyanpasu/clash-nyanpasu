import { expect, test } from 'vitest'
import { render } from 'vitest-browser-react'
import { page } from 'vitest/browser'
import { Platform, PlatformProvider, PlatformWindows } from '../src/react'

test('platform gates use injected facts and do not mount excluded children', async ({
  onTestFinished,
}) => {
  const view = await render(
    <PlatformProvider platform={{ surface: 'browser', clientOs: 'windows' }}>
      <PlatformWindows>
        <span>desktop-only</span>
      </PlatformWindows>
      <Platform when={{ surface: 'browser', clientOs: 'windows' }}>
        <span>browser-windows</span>
      </Platform>
    </PlatformProvider>,
  )
  onTestFinished(() => view.unmount())
  await expect.element(page.getByText('browser-windows')).toBeInTheDocument()
  await expect.element(page.getByText('desktop-only')).not.toBeInTheDocument()
})
