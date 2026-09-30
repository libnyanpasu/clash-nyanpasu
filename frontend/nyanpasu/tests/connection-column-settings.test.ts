import { expect, test } from 'vitest'
import { resolveColumnOrder } from '../src/pages/(main)/main/connections/_modules/column-settings.ts'

const ids = ['Host', 'Chains', 'Process', 'Time']

test('nothing saved keeps the default order', () => {
  expect(resolveColumnOrder([], ids)).toEqual(ids)
})

test('a saved order is kept', () => {
  expect(
    resolveColumnOrder(['Time', 'Host', 'Process', 'Chains'], ids),
  ).toEqual(['Time', 'Host', 'Process', 'Chains'])
})

test('columns added since the order was saved follow it in default order', () => {
  expect(resolveColumnOrder(['Process', 'Host'], ids)).toEqual([
    'Process',
    'Host',
    'Chains',
    'Time',
  ])
})

test('saved ids of columns that are gone, or repeated, are dropped', () => {
  expect(resolveColumnOrder(['Removed', 'Time', 'Time', 'Host'], ids)).toEqual([
    'Time',
    'Host',
    'Chains',
    'Process',
  ])
})
