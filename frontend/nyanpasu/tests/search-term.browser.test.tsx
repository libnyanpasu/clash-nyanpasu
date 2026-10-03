import { AnimatePresence, motion } from 'motion/react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { useSearchTerm } from '../src/pages/(main)/main/_modules/use-search-term'

// Longer than the field's debounce, so a write would have happened.
const settle = () => new Promise((resolve) => setTimeout(resolve, 500))

function Field({
  q,
  writeQuery,
}: {
  q?: string
  writeQuery: (q: string | undefined) => void
}) {
  const [search, setSearch] = useSearchTerm(q, writeQuery)

  return (
    <input
      aria-label="search"
      value={search}
      onChange={(e) => setSearch(e.target.value)}
    />
  )
}

// The page as AnimatedOutlet renders it: kept mounted while it slides out.
function Page({
  present,
  q,
  writeQuery,
}: {
  present: boolean
  q?: string
  writeQuery: (q: string | undefined) => void
}) {
  return (
    <AnimatePresence>
      {present && (
        <motion.div
          key="page"
          exit={{ opacity: 0 }}
          transition={{ duration: 5 }}
        >
          <Field q={q} writeQuery={writeQuery} />
        </motion.div>
      )}
    </AnimatePresence>
  )
}

test('the term is written once typing pauses', async () => {
  const writeQuery = vi.fn()

  const screen = await render(<Page present writeQuery={writeQuery} />)

  await screen.getByLabelText('search').fill('google')

  await expect.poll(() => writeQuery.mock.calls).toEqual([['google']])
})

test('a page sliding out does not write its term', async () => {
  const writeQuery = vi.fn()

  const screen = await render(<Page present writeQuery={writeQuery} />)

  // Typing and leaving within the debounce, as a quick jump away does. A
  // browser fill can take longer than the debounce, so the field is set here.
  const input = screen.getByLabelText('search').element() as HTMLInputElement
  Object.getOwnPropertyDescriptor(
    HTMLInputElement.prototype,
    'value',
  )!.set!.call(input, 'google')
  input.dispatchEvent(new Event('input', { bubbles: true }))
  await screen.rerender(<Page present={false} writeQuery={writeQuery} />)

  await settle()
  expect(screen.getByLabelText('search')).toHaveValue('google')
  expect(writeQuery).not.toHaveBeenCalled()
})

test('another entry’s term replaces the field instead of being overwritten', async () => {
  const writeQuery = vi.fn()

  const screen = await render(
    <Page present q="google" writeQuery={writeQuery} />,
  )

  await screen.getByLabelText('search').fill('')
  await expect.poll(() => writeQuery.mock.calls).toEqual([[undefined]])

  // Back to the entry that still has the earlier term.
  await screen.rerender(<Page present q="github" writeQuery={writeQuery} />)

  await expect.element(screen.getByLabelText('search')).toHaveValue('github')
  await settle()
  expect(writeQuery).toHaveBeenCalledTimes(1)
})
