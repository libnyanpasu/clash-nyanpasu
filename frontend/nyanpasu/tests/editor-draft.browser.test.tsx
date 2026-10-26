import { act } from 'react'
import { expect, test } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import { useDraft } from '../src/pages/(editor)/editor/_modules/hooks'

test('an untouched draft follows the saved content', async () => {
  const hook = await renderHook(
    (props?: { saved: string | undefined }) => useDraft(props?.saved),
    {
      initialProps: { saved: undefined as string | undefined },
    },
  )
  expect(hook.result.current[0]).toBeUndefined()

  await hook.rerender({ saved: 'a: 1' })
  await expect.poll(() => hook.result.current[0]).toBe('a: 1')

  await hook.rerender({ saved: 'a: 2' })
  await expect.poll(() => hook.result.current[0]).toBe('a: 2')

  await hook.rerender({ saved: '' })
  await expect.poll(() => hook.result.current[0]).toBe('')
  await hook.unmount()
})

test('an edited draft stays when the saved content changes', async () => {
  const hook = await renderHook(
    (props?: { saved: string | undefined }) => useDraft(props?.saved),
    {
      initialProps: { saved: 'a: 1' as string | undefined },
    },
  )
  await expect.poll(() => hook.result.current[0]).toBe('a: 1')

  act(() => hook.result.current[1]('a: 1\nb: 2'))
  await hook.rerender({ saved: 'a: 3' })

  await expect.poll(() => hook.result.current[0]).toBe('a: 1\nb: 2')
  await hook.unmount()
})
