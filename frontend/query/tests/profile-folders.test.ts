import { expect, test } from 'vitest'
import type { ProfileItem_Serialize, ProfileLayout } from '@nyanpasu/rpc/types'
import {
  applyProfileLayout,
  profileLayoutOf,
  profileSections,
} from '../src/ipc/profile-folders'

const config = (uid: string): ProfileItem_Serialize => ({
  type: 'config',
  uid,
  name: uid,
  config: {
    type: 'file',
    transforms: [],
    source: {
      type: 'local',
      binding: { type: 'managed', file: `${uid}.yaml` },
    },
  },
})

const transform = (uid: string): ProfileItem_Serialize => ({
  type: 'transform',
  uid,
  name: uid,
  transform: {
    type: 'script',
    runtime: 'javascript',
    source: { type: 'local', binding: { type: 'managed', file: `${uid}.js` } },
  },
})

const uids = (items: { uid: string }[]) => items.map((item) => item.uid)

test('profileSections groups Config profiles by folder and keeps transforms out', () => {
  const source = {
    items: [config('a'), transform('t'), config('b'), config('c'), config('d')],
    folders: [
      { id: 'f1', name: 'Work', members: ['c', 'a'] },
      { id: 'f2', name: 'Empty', members: [] },
    ],
  }

  const sections = profileSections(source)

  expect(sections.folders.map((entry) => entry.folder.id)).toEqual(['f1', 'f2'])
  expect(uids(sections.folders[0].profiles)).toEqual(['c', 'a'])
  expect(sections.folders[1].profiles).toEqual([])
  expect(uids(sections.ungrouped)).toEqual(['b', 'd'])
})

test('profileSections treats a missing folders field as no folders', () => {
  const sections = profileSections({ items: [config('a'), transform('t')] })

  expect(sections.folders).toEqual([])
  expect(uids(sections.ungrouped)).toEqual(['a'])
})

test('profileSections ignores members that are not Config profiles', () => {
  const sections = profileSections({
    items: [config('a'), transform('t')],
    folders: [{ id: 'f1', name: 'Work', members: ['t', 'gone', 'a'] }],
  })

  expect(uids(sections.folders[0].profiles)).toEqual(['a'])
  expect(sections.ungrouped).toEqual([])
})

test('profileLayoutOf reads the layout back from the document', () => {
  const source = {
    items: [config('a'), config('b'), config('c')],
    folders: [{ id: 'f1', name: 'Work', members: ['c'] }],
  }

  expect(profileLayoutOf(source)).toEqual({
    folders: [{ id: 'f1', name: 'Work', members: ['c'] }],
    ungrouped: ['a', 'b'],
  })
})

test('applyProfileLayout orders folders, then ungrouped, then transforms', () => {
  const source = {
    current: 'a',
    valid: [],
    items: [
      config('a'),
      transform('t1'),
      config('b'),
      config('c'),
      transform('t2'),
    ],
    folders: [],
  }
  const layout: ProfileLayout = {
    folders: [
      { id: 'f1', name: 'Work', members: ['c'] },
      { id: 'f2', name: 'Home', members: ['a'] },
    ],
    ungrouped: ['b'],
  }

  const next = applyProfileLayout(source, layout)

  expect(uids(next.items)).toEqual(['c', 'a', 'b', 't1', 't2'])
  expect(next.folders).toEqual(layout.folders)
  expect(next.current).toBe('a')
  expect(uids(source.items)).toEqual(['a', 't1', 'b', 'c', 't2'])
})

test('applyProfileLayout and profileLayoutOf agree', () => {
  const layout: ProfileLayout = {
    folders: [{ id: 'f1', name: 'Work', members: ['b', 'a'] }],
    ungrouped: ['c'],
  }

  const next = applyProfileLayout(
    { items: [config('a'), config('b'), config('c')], folders: [] },
    layout,
  )

  expect(profileLayoutOf(next)).toEqual(layout)
})
