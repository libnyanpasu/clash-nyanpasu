import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { LazyImage } from '../src/lazy-image'

test('image errors end loading and source changes reset loading', async ({
  onTestFinished,
}) => {
  const onError = vi.fn()
  const onLoad = vi.fn()
  const view = await render(
    <LazyImage
      src="data:image/png;base64,AA=="
      className="size-8"
      onError={onError}
      onLoad={onLoad}
    />,
  )
  onTestFinished(async () => {
    await view.unmount()
  })
  const image = () => document.querySelector('img')!
  await expect
    .poll(() =>
      document
        .querySelector('[data-slot="lazy-image-placeholder"]')
        ?.getAttribute('data-error'),
    )
    .toBe('true')
  expect(onError).toHaveBeenCalled()

  await view.rerender(
    <LazyImage
      src={undefined}
      className="size-8"
      onError={onError}
      onLoad={onLoad}
    />,
  )
  await expect
    .poll(() =>
      document
        .querySelector('[data-slot="lazy-image-placeholder"]')
        ?.getAttribute('data-error'),
    )
    .toBeNull()
  expect(
    document
      .querySelector('[data-slot="lazy-image-placeholder"]')
      ?.classList.contains('animate-pulse'),
  ).toBe(true)
  await view.rerender(
    <LazyImage
      src={`data:image/svg+xml,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>')}`}
      className="size-8"
      onError={onError}
      onLoad={onLoad}
    />,
  )
  await expect.poll(() => image().classList.contains('hidden')).toBe(false)
  expect(onLoad).toHaveBeenCalled()
})
