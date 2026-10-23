import { describe, expect, it } from 'vitest'
import {
  buildSteps,
  filterSteps,
  matchField,
  resolveStep,
  stageOf,
  stepStatus,
} from '@/pages/(main)/main/profiles/inspect/_modules/inspect-steps'
import type {
  OperatorTag,
  RuntimeInspectionNode,
  StepLogLevel,
} from '@nyanpasu/rpc/types'

const node = (
  id: number,
  tag: OperatorTag,
  extra: Partial<RuntimeInspectionNode> = {},
): RuntimeInspectionNode => ({
  id,
  tag,
  next: [],
  has_logs: false,
  max_log_level: null,
  changed_fields: [],
  ...extra,
})

const file = (id: number, profile = 'cfg', extra = {}) =>
  node(
    id,
    {
      kind: 'file_config_root',
      data: { profile_id: profile, role: { kind: 'selected' } },
    },
    { changed_fields: null, ...extra },
  )

const global = (id: number, transform: string, index: number, extra = {}) =>
  node(
    id,
    {
      kind: 'global_transform',
      data: {
        selected_profile_id: 'cfg',
        transform_profile_id: transform,
        transform_kind: { type: 'overlay' },
        step_index: index,
      },
    },
    extra,
  )

const builtin = (id: number, step: string, extra = {}) =>
  node(
    id,
    {
      kind: 'builtin_step',
      data: { selected_profile_id: 'cfg', step },
    } as OperatorTag,
    extra,
  )

const scoped = (
  id: number,
  transform: string,
  index: number,
  role: object = { kind: 'selected' },
) =>
  node(id, {
    kind: 'scoped_transform',
    data: {
      host_profile_id: 'cfg',
      role,
      transform_profile_id: transform,
      transform_kind: { type: 'overlay' },
      step_index: index,
    },
  } as unknown as OperatorTag)

/** root, scoped, global, a built-in step that is the final node, then the core. */
const chain = () => [
  file(0),
  scoped(1, 'rename', 0),
  global(2, 'ad-block', 0, { changed_fields: ['rules'] }),
  builtin(3, 'managed_settings', { changed_fields: ['mixed-port'] }),
  builtin(4, 'defaults'),
  builtin(5, 'core_controller'),
]

describe('stageOf', () => {
  it('maps tags to stages and lets the final node win over its tag', () => {
    const nodes = chain()
    const stages = nodes.map((n) => stageOf(n, 4))
    expect(stages).toEqual([
      'source',
      'profile',
      'global',
      'nyanpasu',
      'result',
      'result',
    ])
  })

  it('puts the built-in transform in the Nyanpasu stage', () => {
    const transform = node(7, {
      kind: 'builtin_transform',
      data: { selected_profile_id: null, name: 'x', step_index: 1 },
    })
    expect(stageOf(transform, 99)).toBe('nyanpasu')
  })

  it('treats the core node as a result without being the final node', () => {
    expect(stageOf(builtin(5, 'core_controller'), 4)).toBe('result')
  })
})

describe('buildSteps', () => {
  it('gives final and core their own keys and orders by stage', () => {
    const steps = buildSteps(chain(), 4)
    expect(steps.map((s) => s.key)).toEqual([
      'root:cfg',
      'scoped:cfg:rename:0',
      'global:ad-block:0',
      'builtin:managed_settings',
      'final',
      'core',
    ])
  })

  it('keeps keys when the node ids change', () => {
    const shifted = chain().map((n) => ({ ...n, id: n.id + 10 }))
    expect(buildSteps(shifted, 14).map((s) => s.key)).toEqual(
      buildSteps(chain(), 4).map((s) => s.key),
    )
  })

  it('orders steps by stage even when the nodes interleave them', () => {
    const nodes = [
      file(0),
      global(1, 'a', 0),
      scoped(2, 's', 0),
      builtin(3, 'defaults'),
    ]
    expect(buildSteps(nodes, 3).map((s) => s.stage)).toEqual([
      'source',
      'profile',
      'global',
      'result',
    ])
  })

  it('suffixes keys that are still shared, in node order', () => {
    const nodes = [
      file(0),
      global(1, 't', 0),
      global(2, 't', 0),
      global(3, 't', 0),
      builtin(4, 'defaults'),
    ]
    const keys = buildSteps(nodes, 4).map((s) => s.key)
    expect(keys).toEqual([
      'root:cfg',
      'global:t:0',
      'global:t:0~2',
      'global:t:0~3',
      'final',
    ])
  })

  it('marks scoped transforms that run for a composition', () => {
    const nodes = [
      scoped(0, 't', 0),
      scoped(1, 't', 0, {
        kind: 'composition_base',
        data: { composition_id: 'mix' },
      }),
    ]
    expect(buildSteps(nodes, 9).map((s) => s.key)).toEqual([
      'scoped:cfg:t:0',
      'scoped:cfg:t:0@mix',
    ])
  })

  it('nests composition members under the composition', () => {
    const member = (id: number, profile: string, role: object) =>
      node(
        id,
        {
          kind: 'file_config_root',
          data: { profile_id: profile, role },
        } as OperatorTag,
        { changed_fields: null },
      )
    const nodes = [
      member(0, 'base', {
        kind: 'composition_base',
        data: { composition_id: 'mix' },
      }),
      member(1, 'extra', {
        kind: 'composition_contributor',
        data: { composition_id: 'mix', contributor_index: 0 },
      }),
      node(
        2,
        { kind: 'composition_root', data: { profile_id: 'mix', base: 'base' } },
        { changed_fields: null },
      ),
      node(3, {
        kind: 'extend_proxies_step',
        data: {
          composition_id: 'mix',
          contributor_profile_id: 'extra',
          contributor_index: 0,
        },
      }),
      builtin(4, 'defaults'),
    ]
    const steps = buildSteps(nodes, 4)
    expect(steps.map((s) => [s.key, s.depth])).toEqual([
      ['root:mix', 0],
      ['root:base', 1],
      ['root:extra', 1],
      ['extend:mix:0', 1],
      ['final', 0],
    ])
  })

  it('keeps a composition member at depth 0 when its composition is missing', () => {
    const steps = buildSteps(
      [
        node(0, {
          kind: 'extend_proxies_step',
          data: {
            composition_id: 'gone',
            contributor_profile_id: 'x',
            contributor_index: 0,
          },
        }),
      ],
      9,
    )
    expect(steps[0].depth).toBe(0)
  })

  it('uses the bare root key', () => {
    const steps = buildSteps(
      [node(0, { kind: 'bare_root' }, { changed_fields: null })],
      9,
    )
    expect(steps[0].key).toBe('root:bare')
    expect(steps[0].status).toBe('start')
  })
})

describe('stepStatus', () => {
  const status = (n: RuntimeInspectionNode, finalId = 99) =>
    stepStatus(n, finalId)

  it('reports errors and warnings over every other status', () => {
    for (const level of ['warn', 'error'] as StepLogLevel[]) {
      const withLog = (n: RuntimeInspectionNode) => ({
        ...n,
        max_log_level: level,
      })
      expect(
        status(withLog(global(1, 't', 0, { changed_fields: ['dns'] }))),
      ).toBe(level)
      expect(status(withLog(file(0)))).toBe(level)
      expect(status(withLog(builtin(4, 'defaults')), 4)).toBe(level)
    }
  })

  it('does not treat info and log levels as problems', () => {
    expect(status(global(1, 't', 0, { max_log_level: 'info' }))).toBe(
      'unchanged',
    )
    expect(status(global(1, 't', 0, { max_log_level: 'log' }))).toBe(
      'unchanged',
    )
  })

  it('starts at a root without a baseline', () => {
    expect(status(file(0))).toBe('start')
  })

  it('tells changed from unchanged by the field count', () => {
    expect(status(global(1, 't', 0, { changed_fields: ['dns'] }))).toBe(
      'changed',
    )
    expect(status(global(1, 't', 0, { changed_fields: [] }))).toBe('unchanged')
    // No baseline outside a root is nothing to compare, so nothing changed.
    expect(status(global(1, 't', 0, { changed_fields: null }))).toBe(
      'unchanged',
    )
  })

  it('marks the final and core nodes', () => {
    expect(status(builtin(4, 'defaults'), 4)).toBe('final')
    expect(status(builtin(5, 'core_controller'), 4)).toBe('core')
  })

  it('carries the status and field count on the step', () => {
    const [, , global_] = buildSteps(chain(), 4)
    expect(global_.status).toBe('changed')
    expect(global_.changedCount).toBe(1)
    expect(buildSteps(chain(), 4)[0].changedCount).toBeNull()
  })
})

describe('filterSteps', () => {
  const nodes = [
    file(0),
    scoped(1, 'quiet', 0),
    global(2, 'busy', 0, { changed_fields: ['rules'] }),
    global(3, 'noisy', 0, { max_log_level: 'warn' }),
    global(4, 'broken', 0, { max_log_level: 'error' }),
    global(5, 'idle', 0),
    builtin(6, 'defaults'),
    builtin(7, 'core_controller'),
  ]
  const steps = buildSteps(nodes, 6)

  it('shows everything unless asked', () => {
    expect(filterSteps(steps, { onlyChanged: false })).toEqual(steps)
  })

  it('keeps source, result, changed, warning and error steps', () => {
    const keys = filterSteps(steps, { onlyChanged: true }).map((s) => s.key)
    expect(keys).toEqual([
      'root:cfg',
      'global:busy:0',
      'global:noisy:0',
      'global:broken:0',
      'final',
      'core',
    ])
  })

  it('keeps the selected step', () => {
    const keys = filterSteps(steps, {
      onlyChanged: true,
      selectedKey: 'global:idle:0',
    }).map((s) => s.key)
    expect(keys).toContain('global:idle:0')
    expect(keys).not.toContain('scoped:cfg:quiet:0')
  })
})

describe('resolveStep', () => {
  const steps = buildSteps(chain(), 4)

  it('finds a step by key', () => {
    expect(resolveStep(steps, 'global:ad-block:0')?.node.id).toBe(2)
  })

  it('falls back to the final step for a missing or unknown key', () => {
    expect(resolveStep(steps, undefined)?.key).toBe('final')
    expect(resolveStep(steps, 'global:gone:9')?.key).toBe('final')
  })

  it('falls back to the last step when there is no final key', () => {
    expect(resolveStep(steps.slice(0, 2), 'nope')?.key).toBe(
      'scoped:cfg:rename:0',
    )
    expect(resolveStep([], 'nope')).toBeUndefined()
  })
})

describe('matchField', () => {
  const [, busy] = buildSteps(
    [
      file(0),
      global(1, 't', 0, { changed_fields: ['dns', 'rules', 'mixed-port'] }),
      builtin(2, 'defaults'),
    ],
    2,
  )

  it('matches part of a field name, ignoring case', () => {
    expect(matchField(busy, 'RUL')).toBe(true)
    expect(matchField(busy, 'port')).toBe(true)
    expect(matchField(busy, 'proxies')).toBe(false)
  })

  it('matches a path below a changed field', () => {
    expect(matchField(busy, 'dns.nameserver')).toBe(true)
    expect(matchField(busy, 'rules[0]')).toBe(true)
    expect(matchField(busy, 'dnsx.nameserver')).toBe(false)
  })

  it('never matches an empty query or a step without fields', () => {
    expect(matchField(busy, '')).toBe(false)
    expect(matchField(busy, '  ')).toBe(false)
    expect(
      matchField(buildSteps([file(0), builtin(1, 'defaults')], 1)[0], 'dns'),
    ).toBe(false)
  })
})
