import { describe, expect, it } from 'vitest'
import {
  collapsedSectionId,
  NAV_ALL_ID,
  resolveProfileDrop,
  sectionId,
  sectionItemsOf,
  sidebarFolderId,
  withoutProfile,
} from '@/pages/(main)/main/profiles/_modules/resolve-profile-drop'
import type { ProfileLayout } from '@nyanpasu/rpc/types'

const layout: ProfileLayout = {
  folders: [
    { id: 'f1', name: 'Work', members: ['a', 'b'] },
    { id: 'f2', name: 'Home', members: [] },
  ],
  ungrouped: ['c'],
}

describe('sectionItemsOf', () => {
  it('keys each folder and the ungrouped profiles by section id', () => {
    expect(sectionItemsOf(layout)).toEqual({
      [sectionId('f1')]: ['a', 'b'],
      [sectionId('f2')]: [],
      [sectionId(null)]: ['c'],
    })
  })
})

describe('resolveProfileDrop', () => {
  const items = sectionItemsOf(layout)

  it('appends to the folder of a sidebar item', () => {
    expect(
      resolveProfileDrop({
        uid: 'c',
        targetId: sidebarFolderId('f1'),
        items,
        layout,
      }),
    ).toEqual({ destination: 'f1' })
  })

  it('appends to "not in a folder" on All profiles', () => {
    expect(
      resolveProfileDrop({ uid: 'a', targetId: NAV_ALL_ID, items, layout }),
    ).toEqual({ destination: null })
  })

  it('appends to a collapsed section', () => {
    expect(
      resolveProfileDrop({
        uid: 'c',
        targetId: collapsedSectionId('f2'),
        items,
        layout,
      }),
    ).toEqual({ destination: 'f2' })

    expect(
      resolveProfileDrop({
        uid: 'a',
        targetId: collapsedSectionId(null),
        items,
        layout,
      }),
    ).toEqual({ destination: null })
  })

  it('keeps the place the drag preview shows for a card or a section', () => {
    const preview = {
      [sectionId('f1')]: ['b'],
      [sectionId('f2')]: [],
      [sectionId(null)]: ['c', 'a'],
    }

    expect(
      resolveProfileDrop({ uid: 'a', targetId: 'c', items: preview, layout }),
    ).toEqual({ destination: null, index: 1 })

    expect(
      resolveProfileDrop({
        uid: 'a',
        targetId: sectionId(null),
        items: preview,
        layout,
      }),
    ).toEqual({ destination: null, index: 1 })
  })

  it('reads a folder section as that folder', () => {
    expect(
      resolveProfileDrop({
        uid: 'b',
        targetId: 'a',
        items: {
          [sectionId('f1')]: ['b', 'a'],
          [sectionId('f2')]: [],
          [sectionId(null)]: ['c'],
        },
        layout,
      }),
    ).toEqual({ destination: 'f1', index: 0 })
  })

  it('moves nothing when dropped on no target or on an unknown profile', () => {
    expect(
      resolveProfileDrop({ uid: 'a', targetId: undefined, items, layout }),
    ).toBeNull()

    expect(
      resolveProfileDrop({ uid: 'x', targetId: 'a', items, layout }),
    ).toBeNull()

    expect(
      resolveProfileDrop({ uid: 'a', targetId: 'b', items: null, layout }),
    ).toBeNull()
  })
})

describe('a section shown without a profile', () => {
  it('drops the profile from every section', () => {
    expect(withoutProfile(sectionItemsOf(layout), 'a')).toEqual({
      [sectionId('f1')]: ['b'],
      [sectionId('f2')]: [],
      [sectionId(null)]: ['c'],
    })
  })

  it('places a drop among the profiles the layout keeps', () => {
    // The layout has `a` first in Work, but the list does not show it.
    const shown = withoutProfile(sectionItemsOf(layout), 'a')

    expect(
      resolveProfileDrop({
        uid: 'c',
        targetId: 'b',
        items: {
          ...shown,
          [sectionId('f1')]: ['c', 'b'],
          [sectionId(null)]: [],
        },
        layout,
      }),
    ).toEqual({ destination: 'f1', index: 1 })

    expect(
      resolveProfileDrop({
        uid: 'c',
        targetId: 'b',
        items: {
          ...shown,
          [sectionId('f1')]: ['b', 'c'],
          [sectionId(null)]: [],
        },
        layout,
      }),
    ).toEqual({ destination: 'f1', index: 2 })
  })
})
