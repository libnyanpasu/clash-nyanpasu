import { describe, expect, it } from 'vitest'
import {
  folderDragId,
  folderDragIdsOf,
  folderHeadingId,
  folderOfDragId,
  groupDragId,
  groupOfDragId,
  resolveGroupDrop,
  sectionId,
  sectionItemsOf,
} from '@/pages/(main)/main/proxies/_modules/resolve-group-drop'
import type { ProxyGroupLayout } from '@nyanpasu/rpc/types'

const layout: ProxyGroupLayout = {
  folders: [
    { id: 'f1', name: 'Work', members: ['a', 'b'] },
    { id: 'f2', name: 'Home', members: [] },
  ],
  ungrouped: ['c'],
}

describe('drag ids', () => {
  it('keep a group named like a folder apart from the folder', () => {
    expect(groupDragId('folder:f1')).not.toBe(folderDragId('f1'))
    expect(groupOfDragId(groupDragId('folder:f1'))).toBe('folder:f1')
    expect(groupOfDragId(folderDragId('f1'))).toBeUndefined()
    expect(folderOfDragId(folderDragId('f1'))).toBe('f1')
    expect(folderOfDragId(groupDragId('f1'))).toBeUndefined()
  })
})

describe('sectionItemsOf', () => {
  it('keys each folder and the ungrouped groups by section id', () => {
    expect(sectionItemsOf(layout)).toEqual({
      [sectionId('f1')]: [groupDragId('a'), groupDragId('b')],
      [sectionId('f2')]: [],
      [sectionId(null)]: [groupDragId('c')],
    })
  })

  it('lists the folders in order', () => {
    expect(folderDragIdsOf(layout)).toEqual([
      folderDragId('f1'),
      folderDragId('f2'),
    ])
  })
})

describe('resolveGroupDrop', () => {
  const items = sectionItemsOf(layout)

  it('appends to the folder of a heading, without a place', () => {
    expect(
      resolveGroupDrop({ name: 'c', targetId: folderHeadingId('f1'), items }),
    ).toEqual({ destination: 'f1' })
  })

  it('appends to a heading even when the preview moved the group elsewhere', () => {
    const moved = { ...items, [sectionId('f1')]: [groupDragId('c')] }

    expect(
      resolveGroupDrop({
        name: 'c',
        targetId: folderHeadingId('f2'),
        items: moved,
      }),
    ).toEqual({ destination: 'f2' })
  })

  it('leaves the group where the preview shows it', () => {
    const preview = {
      ...items,
      [sectionId('f1')]: [groupDragId('a'), groupDragId('c'), groupDragId('b')],
      [sectionId(null)]: [],
    }

    expect(
      resolveGroupDrop({
        name: 'c',
        targetId: groupDragId('b'),
        items: preview,
      }),
    ).toEqual({ destination: 'f1', index: 1 })
  })

  it('takes the ungrouped list as no folder', () => {
    const preview = {
      ...items,
      [sectionId('f1')]: [groupDragId('b')],
      [sectionId(null)]: [groupDragId('c'), groupDragId('a')],
    }

    expect(
      resolveGroupDrop({
        name: 'a',
        targetId: groupDragId('c'),
        items: preview,
      }),
    ).toEqual({ destination: null, index: 1 })
  })

  it('takes an empty folder as the group moves into it', () => {
    const preview = {
      ...items,
      [sectionId('f2')]: [groupDragId('c')],
      [sectionId(null)]: [],
    }

    expect(
      resolveGroupDrop({
        name: 'c',
        targetId: sectionId('f2'),
        items: preview,
      }),
    ).toEqual({ destination: 'f2', index: 0 })
  })

  it('moves nothing without a target or a preview', () => {
    expect(
      resolveGroupDrop({ name: 'c', targetId: undefined, items }),
    ).toBeNull()
    expect(
      resolveGroupDrop({ name: 'c', targetId: groupDragId('a'), items: null }),
    ).toBeNull()
    expect(
      resolveGroupDrop({ name: 'zzz', targetId: groupDragId('a'), items }),
    ).toBeNull()
  })
})
