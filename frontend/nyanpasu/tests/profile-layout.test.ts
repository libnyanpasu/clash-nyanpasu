import { describe, expect, it } from 'vitest'
import {
  isSameLayout,
  moveProfileInLayout,
  removeFolderFromLayout,
  renameFolderInLayout,
  reorderFoldersInLayout,
} from '@/pages/(main)/main/profiles/_modules/profile-layout'
import type { ProfileLayout } from '@nyanpasu/rpc/types'

const layout: ProfileLayout = {
  folders: [
    { id: 'f1', name: 'Work', members: ['a', 'b'] },
    { id: 'f2', name: 'Home', members: ['c'] },
  ],
  ungrouped: ['d', 'e'],
}

describe('moveProfileInLayout', () => {
  it('moves a profile to the end of another folder', () => {
    expect(moveProfileInLayout(layout, 'a', 'f2')).toEqual({
      folders: [
        { id: 'f1', name: 'Work', members: ['b'] },
        { id: 'f2', name: 'Home', members: ['c', 'a'] },
      ],
      ungrouped: ['d', 'e'],
    })
  })

  it('inserts at the given index', () => {
    expect(
      moveProfileInLayout(layout, 'd', 'f1', 1).folders[0].members,
    ).toEqual(['a', 'd', 'b'])
  })

  it('reorders inside the same folder', () => {
    expect(
      moveProfileInLayout(layout, 'a', 'f1', 1).folders[0].members,
    ).toEqual(['b', 'a'])
  })

  it('takes a profile out of its folder', () => {
    const next = moveProfileInLayout(layout, 'b', null)

    expect(next.folders[0].members).toEqual(['a'])
    expect(next.ungrouped).toEqual(['d', 'e', 'b'])
  })

  it('leaves the layout alone for an unknown folder', () => {
    expect(moveProfileInLayout(layout, 'a', 'missing')).toBe(layout)
  })

  it('does not change the input', () => {
    const copy = structuredClone(layout)
    moveProfileInLayout(layout, 'a', 'f2')

    expect(layout).toEqual(copy)
  })

  it('is a no-op when a profile moves to where it already is', () => {
    expect(isSameLayout(layout, moveProfileInLayout(layout, 'b', 'f1'))).toBe(
      true,
    )
    expect(isSameLayout(layout, moveProfileInLayout(layout, 'e', null))).toBe(
      true,
    )
  })
})

describe('folder edits', () => {
  it('renames a folder', () => {
    expect(renameFolderInLayout(layout, 'f2', 'Family').folders[1].name).toBe(
      'Family',
    )
  })

  it('moves the members of a removed folder to the end of ungrouped', () => {
    expect(removeFolderFromLayout(layout, 'f1')).toEqual({
      folders: [{ id: 'f2', name: 'Home', members: ['c'] }],
      ungrouped: ['d', 'e', 'a', 'b'],
    })
  })

  it('ignores removing an unknown folder', () => {
    expect(removeFolderFromLayout(layout, 'missing')).toBe(layout)
  })

  it('reorders folders and keeps the ones not listed', () => {
    expect(
      reorderFoldersInLayout(layout, ['f2']).folders.map(({ id }) => id),
    ).toEqual(['f2', 'f1'])
    expect(
      reorderFoldersInLayout(layout, ['f2', 'f1', 'x']).folders.map(
        ({ id }) => id,
      ),
    ).toEqual(['f2', 'f1'])
  })
})
