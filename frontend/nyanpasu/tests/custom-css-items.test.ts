import { describe, expect, test, vi } from 'vitest'
import {
  addItem,
  EMPTY_CUSTOM_CSS_STATE,
  LEGACY_CUSTOM_CSS_ITEM_ID,
  migrateLegacyCustomCss,
  removeItem,
  reorderItems,
  resolveActiveStyles,
  setPresetEnabled,
  updateItem,
  type CustomCssItem,
  type CustomCssPreset,
  type CustomCssState,
} from '@/utils/custom-css-items'

function item(id: string, patch: Partial<CustomCssItem> = {}): CustomCssItem {
  return {
    id,
    name: id,
    enabled: true,
    source: `/* ${id} */`,
    compiled: `.${id} {}`,
    ...patch,
  }
}

function preset(id: string): CustomCssPreset {
  return { id, name: () => id, description: () => id, source: `.${id} {}` }
}

function stateOf(...items: CustomCssItem[]): CustomCssState {
  return { ...EMPTY_CUSTOM_CSS_STATE, items }
}

const compile = async (preset: CustomCssPreset) => `compiled:${preset.id}`

describe('migrating the legacy custom CSS', () => {
  test('carries a written string over as one enabled item', () => {
    const state = migrateLegacyCustomCss('a {}', '.a {}', 'Custom CSS')

    expect(state.items).toEqual([
      {
        id: LEGACY_CUSTOM_CSS_ITEM_ID,
        name: 'Custom CSS',
        enabled: true,
        source: 'a {}',
        compiled: '.a {}',
      },
    ])
  })

  test.each([null, '', '  \n'])('creates nothing for %j', (source) => {
    expect(migrateLegacyCustomCss(source, '.a {}', 'Custom CSS')).toEqual(
      EMPTY_CUSTOM_CSS_STATE,
    )
  })
})

describe('editing items', () => {
  test('adds to the end, updates one item and removes by id', () => {
    let state = addItem(stateOf(item('a')), item('b'))
    expect(state.items.map((it) => it.id)).toEqual(['a', 'b'])

    state = updateItem(state, 'b', { enabled: false, name: 'B' })
    expect(state.items[1]).toMatchObject({ enabled: false, name: 'B' })
    expect(state.items[0].enabled).toBe(true)

    state = removeItem(state, 'a')
    expect(state.items.map((it) => it.id)).toEqual(['b'])
  })

  test('reorders by id, keeping unlisted items after and ignoring unknown ids', () => {
    const state = stateOf(item('a'), item('b'), item('c'))

    expect(reorderItems(state, ['c', 'a']).items.map((it) => it.id)).toEqual([
      'c',
      'a',
      'b',
    ])
    expect(
      reorderItems(state, ['b', 'gone', 'a', 'c']).items.map((it) => it.id),
    ).toEqual(['b', 'a', 'c'])
  })

  test('enables a preset once and disables it again', () => {
    let state = setPresetEnabled(EMPTY_CUSTOM_CSS_STATE, 'p', true)
    state = setPresetEnabled(state, 'p', true)
    expect(state.enabledPresets).toEqual(['p'])

    state = setPresetEnabled(state, 'p', false)
    expect(state.enabledPresets).toEqual([])
  })
})

describe('resolving the active styles', () => {
  test('applies several enabled items in list order', async () => {
    const styles = await resolveActiveStyles(
      stateOf(item('a'), item('b', { enabled: false }), item('c')),
      [],
      compile,
    )

    expect(styles.map((style) => style.css)).toEqual(['.a {}', '.c {}'])
  })

  test('puts enabled presets, in catalog order, before the items', async () => {
    const state: CustomCssState = {
      items: [item('a')],
      enabledPresets: ['two', 'one', 'unknown'],
    }

    const styles = await resolveActiveStyles(
      state,
      [preset('one'), preset('two'), preset('off')],
      compile,
    )

    expect(styles.map((style) => style.css)).toEqual([
      'compiled:one',
      'compiled:two',
      '.a {}',
    ])
  })

  test('skips an item that has no compiled CSS', async () => {
    const styles = await resolveActiveStyles(
      stateOf(item('a', { compiled: '' })),
      [],
      compile,
    )

    expect(styles).toEqual([])
  })

  test('reports a preset that fails to compile and still applies the rest', async () => {
    const onError = vi.fn()
    const state: CustomCssState = {
      items: [item('a')],
      enabledPresets: ['bad', 'good'],
    }

    const styles = await resolveActiveStyles(
      state,
      [preset('bad'), preset('good')],
      async (target) => {
        if (target.id === 'bad') throw new Error('syntax')
        return `compiled:${target.id}`
      },
      onError,
    )

    expect(styles.map((style) => style.css)).toEqual(['compiled:good', '.a {}'])
    expect(onError).toHaveBeenCalledOnce()
    expect(onError.mock.calls[0][0].id).toBe('bad')
  })

  test('gives presets and items distinct style ids', async () => {
    const styles = await resolveActiveStyles(
      { items: [item('x')], enabledPresets: ['x'] },
      [preset('x')],
      compile,
    )

    expect(new Set(styles.map((style) => style.id)).size).toBe(2)
  })
})
