import assert from 'node:assert/strict'
import { test } from 'node:test'
import { dependencyViolations } from './backend-boundaries.ts'

test('transitive GUI and host dependencies invalidate neutral packages', () => {
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-config',
      'nyanpasu-config v0.1.0\nnyanpasu-egui v0.1.0\neframe v0.36.1 (*)\n',
    ),
    [
      'nyanpasu-config must not depend on nyanpasu-egui',
      'nyanpasu-config must not depend on eframe',
    ],
  )
  assert.deepStrictEqual(
    dependencyViolations('nyanpasu-platform', 'tauri-specta v2.0.0\n'),
    ['nyanpasu-platform must not depend on tauri-specta'],
  )
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-core',
      'nyanpasu-widget v0.1.0\nslint v1.18.1\nslint-interpreter v1.18.1\ni-slint-core v1.18.1\nwinit v0.30.13\n',
    ),
    [
      'nyanpasu-core must not depend on nyanpasu-widget',
      'nyanpasu-core must not depend on slint',
      'nyanpasu-core must not depend on slint-interpreter',
      'nyanpasu-core must not depend on i-slint-core',
      'nyanpasu-core must not depend on winit',
    ],
  )
})

test('platform may depend on core to implement its ports', () => {
  const graph = 'nyanpasu-platform v0.1.0\nnyanpasu-core v0.1.0\n'
  assert.deepStrictEqual(dependencyViolations('nyanpasu-platform', graph), [])
})

test('core must not depend on platform implementations', () => {
  const graph = 'nyanpasu-core v0.1.0\nnyanpasu-platform v0.1.0\n'
  assert.deepStrictEqual(dependencyViolations('nyanpasu-core', graph), [
    'nyanpasu-core must not depend on nyanpasu-platform',
  ])
})

test('core may depend on foundation packages', () => {
  assert.deepStrictEqual(
    dependencyViolations('nyanpasu-core', 'nyanpasu-config v0.1.0\n'),
    [],
  )
})

test('neutral packages cannot depend on the crash reporter frontend', () => {
  for (const owner of [
    'nyanpasu-core',
    'nyanpasu-platform',
    'nyanpasu-config',
    'nyanpasu-diagnose',
  ]) {
    assert.deepStrictEqual(
      dependencyViolations(owner, 'nyanpasu-crash-reporter v0.1.0\n'),
      [`${owner} must not depend on nyanpasu-crash-reporter`],
    )
  }
})

test('domain, actor, script and storage dependencies remain available', () => {
  for (const owner of [
    'nyanpasu-core',
    'nyanpasu-platform',
    'nyanpasu-config',
    'nyanpasu-diagnose',
  ]) {
    assert.deepStrictEqual(
      dependencyViolations(
        owner,
        'nyanpasu-config v0.1.0\nnyanpasu-widget-protocol v0.1.0\nractor v0.16\nboa_engine v0.22\nmlua v0.12\ntokio v1.0\n',
      ),
      [],
    )
  }
})

test('diagnose must not depend on core or platform', () => {
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-diagnose',
      'nyanpasu-core v0.1.0\nnyanpasu-platform v0.1.0\n',
    ),
    [
      'nyanpasu-diagnose must not depend on nyanpasu-core',
      'nyanpasu-diagnose must not depend on nyanpasu-platform',
    ],
  )
})

test('diagnose consumes config and paths without application services', () => {
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-diagnose',
      'nyanpasu-config v0.1.0\nnyanpasu-paths v0.1.0\n',
    ),
    [],
  )
})

test('the root package itself is not a violation', () => {
  assert.deepStrictEqual(
    dependencyViolations('nyanpasu-platform', 'nyanpasu-platform v0.1.0\n', [
      { package: 'nyanpasu-platform', forbids: ['nyanpasu-platform'] },
    ]),
    [],
  )
})

test('forbidding nyanpasu-local-channel catches the widget ipc feature', () => {
  const rules = [
    { package: 'nyanpasu-config', forbids: ['nyanpasu-local-channel'] },
  ]
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-config',
      'nyanpasu-config v0.1.0\nnyanpasu-widget-protocol v0.1.0\n',
      rules,
    ),
    [],
  )
  // nyanpasu-widget-protocol's ipc feature enables nyanpasu-local-channel.
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-config',
      'nyanpasu-config v0.1.0\nnyanpasu-widget-protocol v0.1.0\nnyanpasu-local-channel v0.1.0\n',
      rules,
    ),
    ['nyanpasu-config must not depend on nyanpasu-local-channel'],
  )
})
