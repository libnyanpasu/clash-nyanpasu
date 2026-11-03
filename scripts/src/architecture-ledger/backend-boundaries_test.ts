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
    dependencyViolations(
      'nyanpasu-core',
      'nyanpasu-config v0.1.0\nnyanpasu-contracts v0.1.0\nnyanpasu-profile-transform v0.1.0\n',
    ),
    [],
  )
})

test('neutral packages cannot depend on the crash reporter frontend', () => {
  for (const owner of [
    'nyanpasu-core',
    'nyanpasu-platform',
    'nyanpasu-config',
    'nyanpasu-contracts',
    'nyanpasu-profile-transform',
    'nyanpasu-scripts',
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

test('foundation packages must not depend on application or platform packages', () => {
  for (const owner of [
    'nyanpasu-config',
    'nyanpasu-contracts',
    'nyanpasu-profile-transform',
  ]) {
    assert.deepStrictEqual(
      dependencyViolations(
        owner,
        'nyanpasu-core v0.1.0\nnyanpasu-platform v0.1.0\n',
      ),
      [
        `${owner} must not depend on nyanpasu-core`,
        `${owner} must not depend on nyanpasu-platform`,
      ],
    )
    assert.deepStrictEqual(
      dependencyViolations(owner, 'nyanpasu-paths v0.1.0\nserde v1.0\n'),
      [],
    )
  }
})

test('config must not depend on other foundation packages', () => {
  assert.deepStrictEqual(
    dependencyViolations('nyanpasu-config', 'nyanpasu-contracts v0.1.0\n'),
    ['nyanpasu-config must not depend on nyanpasu-contracts'],
  )
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-config',
      'nyanpasu-config v0.1.0\nnyanpasu-paths v0.1.0\n',
    ),
    [],
  )
})

test('contracts excludes actor, HTTP, database and core-control dependencies', () => {
  for (const dependency of [
    'ractor',
    'reqwest',
    'redb',
    'nyanpasu-core-manager',
  ]) {
    assert.deepStrictEqual(
      dependencyViolations('nyanpasu-contracts', `${dependency} v0.1.0\n`),
      [`nyanpasu-contracts must not depend on ${dependency}`],
    )
  }
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-contracts',
      'nyanpasu-contracts v0.1.0\nnyanpasu-config v0.1.0\nserde v1.0\n',
    ),
    [],
  )
})

test('forbidding nyanpasu-local-channel catches the widget ipc feature', () => {
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-contracts',
      'nyanpasu-contracts v0.1.0\nnyanpasu-widget-protocol v0.1.0\n',
    ),
    [],
  )
  // nyanpasu-widget-protocol's ipc feature enables nyanpasu-local-channel.
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-contracts',
      'nyanpasu-contracts v0.1.0\nnyanpasu-widget-protocol v0.1.0\nnyanpasu-local-channel v0.1.0\n',
    ),
    ['nyanpasu-contracts must not depend on nyanpasu-local-channel'],
  )
})

test('script implementations reject GUI dependencies and accept script engines', () => {
  assert.deepStrictEqual(
    dependencyViolations('nyanpasu-scripts', 'tauri v2.0.0\nslint v1.18.1\n'),
    [
      'nyanpasu-scripts must not depend on tauri',
      'nyanpasu-scripts must not depend on slint',
    ],
  )
  assert.deepStrictEqual(
    dependencyViolations('nyanpasu-scripts', 'boa_engine v0.22\nmlua v0.12\n'),
    [],
  )
})

test('implementations reject core and transitional platform dependencies', () => {
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-scripts',
      'nyanpasu-core v0.1.0\nnyanpasu-platform v0.1.0\n',
    ),
    [
      'nyanpasu-scripts must not depend on nyanpasu-core',
      'nyanpasu-scripts must not depend on nyanpasu-platform',
    ],
  )
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-scripts',
      'nyanpasu-scripts v0.1.0\nnyanpasu-config v0.1.0\nnyanpasu-contracts v0.1.0\n',
    ),
    [],
  )
})

test('foundation, core, diagnose and platform reject implementation dependencies', () => {
  for (const owner of [
    'nyanpasu-config',
    'nyanpasu-contracts',
    'nyanpasu-profile-transform',
    'nyanpasu-core',
    'nyanpasu-diagnose',
    'nyanpasu-platform',
  ]) {
    assert.deepStrictEqual(
      dependencyViolations(owner, 'nyanpasu-scripts v0.1.0\n'),
      [`${owner} must not depend on nyanpasu-scripts`],
    )
    assert.deepStrictEqual(
      dependencyViolations(owner, 'nyanpasu-paths v0.1.0\nserde v1.0\n'),
      [],
    )
  }
})

test('config rejects transform and accepts lower-level libraries', () => {
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-config',
      'nyanpasu-profile-transform v0.1.0\n',
    ),
    ['nyanpasu-config must not depend on nyanpasu-profile-transform'],
  )
  assert.deepStrictEqual(
    dependencyViolations('nyanpasu-config', 'serde v1.0\n'),
    [],
  )
})

test('transform rejects contracts and persistence and accepts config', () => {
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-profile-transform',
      'nyanpasu-contracts v0.1.0\nnyanpasu-persistence v0.1.0\n',
    ),
    [
      'nyanpasu-profile-transform must not depend on nyanpasu-contracts',
      'nyanpasu-profile-transform must not depend on nyanpasu-persistence',
    ],
  )
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-profile-transform',
      'nyanpasu-config v0.1.0\n',
    ),
    [],
  )
})

test('contracts rejects transform and accepts config', () => {
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-contracts',
      'nyanpasu-profile-transform v0.1.0\n',
    ),
    ['nyanpasu-contracts must not depend on nyanpasu-profile-transform'],
  )
  assert.deepStrictEqual(
    dependencyViolations('nyanpasu-contracts', 'nyanpasu-config v0.1.0\n'),
    [],
  )
})
