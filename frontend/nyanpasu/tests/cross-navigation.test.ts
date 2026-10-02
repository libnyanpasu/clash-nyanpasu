import { expect, test } from 'vitest'
import {
  historyDirection,
  keepReturn,
  returnStep,
} from '../src/components/router/cross-navigation'

test('a ticket behind the current entry goes back by the gap', () => {
  expect(
    returnStep({ page: 'rules', href: '/main/rules', index: 3 }, 6),
  ).toEqual({ kind: 'go', delta: -3 })
})

test('a ticket not behind the current entry navigates to its href', () => {
  expect(
    returnStep({ page: 'rules', href: '/main/rules?q=x', index: 6 }, 6),
  ).toEqual({ kind: 'href', href: '/main/rules?q=x' })
})

test('in-page navigations keep the way back and drop the focus', () => {
  const ticket = { page: 'traffic' as const, href: '/main/topology', index: 1 }

  expect(keepReturn({ __TSR_index: 4, returnTo: ticket, focus: 'x' })).toEqual({
    returnTo: ticket,
  })
})

test('only a lower history index is a step back', () => {
  expect(historyDirection(5, 2)).toBe(-1)
  expect(historyDirection(2, 3)).toBeUndefined()
  expect(historyDirection(3, 3)).toBeUndefined()
})
