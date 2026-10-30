import { expect, test } from 'vitest'
import {
  DEFAULT_INSPECT_VIEW,
  toInspectView,
} from '@/pages/(main)/main/profiles/inspect/_modules/inspect-view'

test('a stored inspect view is coerced', () => {
  expect(toInspectView(null)).toEqual(DEFAULT_INSPECT_VIEW)
  expect(toInspectView({ documentTab: 'output', onlyChanged: true })).toEqual({
    documentTab: 'output',
    onlyChanged: true,
  })
  // The logs tab is never kept, and an unknown value falls back.
  expect(toInspectView({ documentTab: 'logs', onlyChanged: 'yes' })).toEqual(
    DEFAULT_INSPECT_VIEW,
  )
})
