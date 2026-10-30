import { expect, test } from 'vitest'
import {
  locateGroup,
  moveGroupByInLayout,
  moveGroupFolderByInLayout,
  moveGroupInLayout,
  removeGroupFolderFromLayout,
  renameGroupFolderInLayout,
  reorderGroupFoldersInLayout,
} from '@/pages/(main)/main/proxies/_modules/group-layout'
import type { ProxyGroupLayout } from '@nyanpasu/rpc/types'

const layout: ProxyGroupLayout = {
  folders: [
    { id: 'f1', name: 'Streaming', members: ['Netflix', 'Disney+'] },
    { id: 'f2', name: 'Regions', members: ['JP'] },
  ],
  ungrouped: ['Proxy', 'Auto'],
}

test('moving a group into a folder takes it out of where it was', () => {
  expect(moveGroupInLayout(layout, 'Proxy', 'f2', 0)).toEqual({
    folders: [
      layout.folders[0],
      { ...layout.folders[1], members: ['Proxy', 'JP'] },
    ],
    ungrouped: ['Auto'],
  })
  expect(moveGroupInLayout(layout, 'Netflix', 'f2').folders).toEqual([
    { ...layout.folders[0], members: ['Disney+'] },
    { ...layout.folders[1], members: ['JP', 'Netflix'] },
  ])
})

test('moving a group out of its folder puts it among the groups', () => {
  expect(moveGroupInLayout(layout, 'Netflix', null, 1)).toEqual({
    folders: [
      { ...layout.folders[0], members: ['Disney+'] },
      layout.folders[1],
    ],
    ungrouped: ['Proxy', 'Netflix', 'Auto'],
  })
})

test('moving a group within its list reorders it', () => {
  expect(moveGroupInLayout(layout, 'Auto', null, 0).ungrouped).toEqual([
    'Auto',
    'Proxy',
  ])
})

test('moving to an unknown folder changes nothing', () => {
  expect(moveGroupInLayout(layout, 'Proxy', 'missing')).toBe(layout)
})

test('moving a group by one stays inside its list or folder', () => {
  expect(moveGroupByInLayout(layout, 'Auto', -1).ungrouped).toEqual([
    'Auto',
    'Proxy',
  ])
  expect(moveGroupByInLayout(layout, 'Netflix', 1).folders[0].members).toEqual([
    'Disney+',
    'Netflix',
  ])
  // The first of a list cannot move up, and the last cannot move down.
  expect(moveGroupByInLayout(layout, 'Proxy', -1)).toEqual(layout)
  expect(moveGroupByInLayout(layout, 'Disney+', 1)).toEqual(layout)
  expect(moveGroupByInLayout(layout, 'Missing', 1)).toEqual(layout)
})

test('renaming changes the folder name only', () => {
  expect(renameGroupFolderInLayout(layout, 'f2', 'Asia')).toEqual({
    ...layout,
    folders: [layout.folders[0], { ...layout.folders[1], name: 'Asia' }],
  })
})

test('removing a folder returns its groups after the existing ones', () => {
  expect(removeGroupFolderFromLayout(layout, 'f1')).toEqual({
    folders: [layout.folders[1]],
    ungrouped: ['Proxy', 'Auto', 'Netflix', 'Disney+'],
  })
  expect(removeGroupFolderFromLayout(layout, 'missing')).toBe(layout)
})

test('moving a folder by one swaps it with its neighbour', () => {
  expect(
    moveGroupFolderByInLayout(layout, 'f1', 1).folders.map(({ id }) => id),
  ).toEqual(['f2', 'f1'])
  expect(moveGroupFolderByInLayout(layout, 'f1', -1)).toBe(layout)
  expect(moveGroupFolderByInLayout(layout, 'f2', 1)).toBe(layout)
})

test('a group is located in its list or folder', () => {
  expect(locateGroup(layout, 'Auto')).toEqual({
    folder: null,
    index: 1,
    count: 2,
  })
  expect(locateGroup(layout, 'Disney+')).toEqual({
    folder: layout.folders[0],
    index: 1,
    count: 2,
  })
  expect(locateGroup(layout, 'Missing')).toBeUndefined()
})

test('folders take the order of the ids and keep any the ids leave out', () => {
  expect(
    reorderGroupFoldersInLayout(layout, ['f2', 'f1']).folders.map(
      (folder) => folder.id,
    ),
  ).toEqual(['f2', 'f1'])
  expect(
    reorderGroupFoldersInLayout(layout, ['f2', 'gone']).folders.map(
      (folder) => folder.id,
    ),
  ).toEqual(['f2', 'f1'])
  expect(reorderGroupFoldersInLayout(layout, ['f2', 'f1']).ungrouped).toEqual(
    layout.ungrouped,
  )
})
