import assert from 'node:assert/strict'
import { test } from 'node:test'
import { dependencyViolations } from './backend-boundaries.ts'

test('the desktop shell constructs OS implementations over foundation contracts', () => {
  assert.deepStrictEqual(
    dependencyViolations(
      'clash-nyanpasu',
      'nyanpasu-os v0.1.0\nsysproxy v0.3\nrunas v1.2\n',
    ),
    [],
  )
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-os',
      'nyanpasu-os v0.1.0\nnyanpasu-contracts v0.1.0\nnyanpasu-config v0.1.0\nnyanpasu-utils v0.1.1\nsysproxy v0.3\nreqwest v0.13\nrunas v1.2\n',
    ),
    [],
  )
})

for (const owner of [
  'nyanpasu-core',
  'nyanpasu-migration',
  'nyanpasu-owner-support',
  'nyanpasu-observers',
  'nyanpasu-updater',
]) {
  test(`${owner} and OS implementations cannot depend on each other`, () => {
    assert.deepStrictEqual(
      dependencyViolations(owner, 'nyanpasu-os v0.1.0\n'),
      [`${owner} must not depend on nyanpasu-os`],
    )
    assert.deepStrictEqual(
      dependencyViolations('nyanpasu-os', `${owner} v0.1.0\n`),
      [`nyanpasu-os must not depend on ${owner}`],
    )
  })
  test(`${owner} rejects direct and transitive sysproxy dependencies`, () => {
    for (const path of [
      'sysproxy v0.3\n',
      'neutral-adapter v0.1\nsysproxy v0.3\n',
    ]) {
      assert.deepStrictEqual(dependencyViolations(owner, path), [
        `${owner} must not depend on sysproxy`,
      ])
    }
  })
  test(`${owner} rejects direct and transitive runas dependencies`, () => {
    for (const path of ['runas v1.2\n', 'neutral-adapter v0.1\nrunas v1.2\n']) {
      assert.deepStrictEqual(dependencyViolations(owner, path), [
        `${owner} must not depend on runas`,
      ])
    }
  })
}

for (const owner of [
  'nyanpasu-config',
  'nyanpasu-contracts',
  'nyanpasu-persistence',
  'nyanpasu-profile-transform',
  'nyanpasu-net',
  'nyanpasu-profile-io',
  'nyanpasu-scripts',
]) {
  test(`${owner} cannot depend on OS implementations`, () => {
    assert.deepStrictEqual(
      dependencyViolations(owner, 'nyanpasu-os v0.1.0\n'),
      [`${owner} must not depend on nyanpasu-os`],
    )
  })
}

for (const dependency of [
  'nyanpasu-ipc',
  'nyanpasu-core-manager',
  'nyanpasu-net',
  'nyanpasu-profile-io',
  'nyanpasu-scripts',
  'clash-nyanpasu',
  'tauri',
  'tauri-plugin-shell',
  'egui',
  'eframe',
  'slint',
  'slint-interpreter',
  'i-slint-core',
]) {
  test(`OS rejects direct and transitive ${dependency} dependencies`, () => {
    for (const prefix of ['', 'neutral-adapter v0.1\n']) {
      assert.deepStrictEqual(
        dependencyViolations('nyanpasu-os', `${prefix}${dependency} v0.1\n`),
        [`nyanpasu-os must not depend on ${dependency}`],
      )
    }
  })
}

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
    dependencyViolations('nyanpasu-core', 'tauri v2.4.0\n'),
    ['nyanpasu-core must not depend on tauri'],
  )
  assert.deepStrictEqual(
    dependencyViolations('nyanpasu-core', 'tauri-specta v2.0.0\n'),
    ['nyanpasu-core must not depend on tauri-specta'],
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

test('core may consume render-free widget protocol through config', () => {
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-core',
      'nyanpasu-config v0.1.0\nnyanpasu-widget-protocol v0.1.0\nnyanpasu-contracts v0.1.0\nnyanpasu-profile-transform v0.1.0\n',
    ),
    [],
  )
})

test('migration consumes foundation packages without storage or core control', () => {
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-migration',
      'nyanpasu-migration v0.1.0\nnyanpasu-config v0.1.0\nnyanpasu-contracts v0.1.0\nnyanpasu-persistence v0.1.0\nnyanpasu-utils v0.1.1\n',
    ),
    [],
  )
  for (const dependency of [
    'redb',
    'nyanpasu-core-manager',
    'nyanpasu-scripts',
    'nyanpasu-profile-io',
    'nyanpasu-net',
    'tauri',
    'slint',
  ]) {
    assert.deepStrictEqual(
      dependencyViolations('nyanpasu-migration', `${dependency} v0.1.0\n`),
      [`nyanpasu-migration must not depend on ${dependency}`],
    )
  }
})

test('migration is a core-family dependency, available to core only above foundation', () => {
  assert.deepStrictEqual(
    dependencyViolations('nyanpasu-core', 'nyanpasu-migration v0.1.0\n'),
    [],
  )
  for (const owner of [
    'nyanpasu-config',
    'nyanpasu-contracts',
    'nyanpasu-persistence',
    'nyanpasu-profile-transform',
    'nyanpasu-scripts',
    'nyanpasu-profile-io',
    'nyanpasu-net',
    'nyanpasu-diagnose',
  ]) {
    assert.deepStrictEqual(
      dependencyViolations(owner, 'nyanpasu-migration v0.1.0\n'),
      [`${owner} must not depend on nyanpasu-migration`],
    )
    assert.deepStrictEqual(dependencyViolations(owner, 'serde v1.0\n'), [])
  }
})

test('owner support consumes actor runtimes without higher core layers or implementations', () => {
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-owner-support',
      'nyanpasu-owner-support v0.1.0\nractor v0.16\ntokio v1.54.1\ntokio-util v0.7\n',
    ),
    [],
  )
  for (const dependency of [
    'nyanpasu-core',
    'nyanpasu-migration',
    'nyanpasu-scripts',
    'nyanpasu-profile-io',
    'nyanpasu-net',
    'tauri',
    'slint',
  ]) {
    assert.deepStrictEqual(
      dependencyViolations('nyanpasu-owner-support', `${dependency} v0.1.0\n`),
      [`nyanpasu-owner-support must not depend on ${dependency}`],
    )
  }
})

test('owner support is available to higher core layers only above foundation', () => {
  for (const owner of ['nyanpasu-core', 'nyanpasu-migration']) {
    assert.deepStrictEqual(
      dependencyViolations(owner, 'nyanpasu-owner-support v0.1.0\n'),
      [],
    )
  }
  for (const owner of [
    'nyanpasu-config',
    'nyanpasu-contracts',
    'nyanpasu-persistence',
    'nyanpasu-profile-transform',
    'nyanpasu-scripts',
    'nyanpasu-profile-io',
    'nyanpasu-net',
    'nyanpasu-diagnose',
  ]) {
    assert.deepStrictEqual(
      dependencyViolations(owner, 'nyanpasu-owner-support v0.1.0\n'),
      [`${owner} must not depend on nyanpasu-owner-support`],
    )
    assert.deepStrictEqual(dependencyViolations(owner, 'serde v1.0\n'), [])
  }
})

test('observers consume domain and owner support without concrete infrastructure', () => {
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-observers',
      'nyanpasu-observers v0.1.0\nnyanpasu-config v0.1.0\nnyanpasu-owner-support v0.1.0\nractor v0.16\ntokio v1.54.1\n',
    ),
    [],
  )
  for (const dependency of [
    'redb',
    'clash-api',
    'nyanpasu-core-manager',
    'notify',
    'notify-debouncer-full',
    'nyanpasu-scripts',
    'nyanpasu-profile-io',
    'nyanpasu-net',
    'tauri',
    'slint',
  ]) {
    assert.deepStrictEqual(
      dependencyViolations('nyanpasu-observers', `${dependency} v0.1.0\n`),
      [`nyanpasu-observers must not depend on ${dependency}`],
    )
  }
})

test('observers are available to core above foundation and owner support', () => {
  assert.deepStrictEqual(
    dependencyViolations('nyanpasu-core', 'nyanpasu-observers v0.1.0\n'),
    [],
  )
  for (const owner of [
    'nyanpasu-config',
    'nyanpasu-contracts',
    'nyanpasu-persistence',
    'nyanpasu-profile-transform',
    'nyanpasu-owner-support',
    'nyanpasu-scripts',
    'nyanpasu-profile-io',
    'nyanpasu-net',
    'nyanpasu-diagnose',
  ]) {
    assert.deepStrictEqual(
      dependencyViolations(owner, 'nyanpasu-observers v0.1.0\n'),
      [`${owner} must not depend on nyanpasu-observers`],
    )
    assert.deepStrictEqual(dependencyViolations(owner, 'serde v1.0\n'), [])
  }
})

test('updater consumes foundation, owner support, HTTP and archive libraries', () => {
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-updater',
      'nyanpasu-updater v0.1.0\nnyanpasu-config v0.1.0\nnyanpasu-contracts v0.1.0\nnyanpasu-owner-support v0.1.0\nnyanpasu-paths v0.1.0\nreqwest v0.13\nzip v9.0\ntar v0.4\nflate2 v1.0\nbolt-load v0.1\n',
    ),
    [],
  )
  for (const dependency of [
    'nyanpasu-scripts',
    'nyanpasu-profile-io',
    'nyanpasu-net',
    'tauri',
    'slint',
  ]) {
    assert.deepStrictEqual(
      dependencyViolations('nyanpasu-updater', `${dependency} v0.1.0\n`),
      [`nyanpasu-updater must not depend on ${dependency}`],
    )
  }
})

test('updater is available to core above foundation and owner support', () => {
  assert.deepStrictEqual(
    dependencyViolations('nyanpasu-core', 'nyanpasu-updater v0.1.0\n'),
    [],
  )
  for (const owner of [
    'nyanpasu-config',
    'nyanpasu-contracts',
    'nyanpasu-persistence',
    'nyanpasu-profile-transform',
    'nyanpasu-owner-support',
    'nyanpasu-scripts',
    'nyanpasu-profile-io',
    'nyanpasu-net',
    'nyanpasu-diagnose',
  ]) {
    assert.deepStrictEqual(
      dependencyViolations(owner, 'nyanpasu-updater v0.1.0\n'),
      [`${owner} must not depend on nyanpasu-updater`],
    )
    assert.deepStrictEqual(dependencyViolations(owner, 'serde v1.0\n'), [])
  }
})

test('neutral packages cannot depend on the crash reporter frontend', () => {
  for (const owner of [
    'nyanpasu-core',
    'nyanpasu-config',
    'nyanpasu-contracts',
    'nyanpasu-persistence',
    'nyanpasu-profile-transform',
    'nyanpasu-scripts',
    'nyanpasu-profile-io',
    'nyanpasu-net',
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

test('diagnose must not depend on core', () => {
  assert.deepStrictEqual(
    dependencyViolations('nyanpasu-diagnose', 'nyanpasu-core v0.1.0\n'),
    ['nyanpasu-diagnose must not depend on nyanpasu-core'],
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
    dependencyViolations('nyanpasu-core', 'nyanpasu-core v0.1.0\n', [
      { package: 'nyanpasu-core', forbids: ['nyanpasu-core'] },
    ]),
    [],
  )
})

test('foundation packages must not depend on application packages', () => {
  for (const owner of [
    'nyanpasu-config',
    'nyanpasu-contracts',
    'nyanpasu-persistence',
    'nyanpasu-profile-transform',
  ]) {
    assert.deepStrictEqual(
      dependencyViolations(owner, 'nyanpasu-core v0.1.0\n'),
      [`${owner} must not depend on nyanpasu-core`],
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
    'nyanpasu-updater',
  ]) {
    assert.deepStrictEqual(
      dependencyViolations('nyanpasu-contracts', `${dependency} v0.1.0\n`),
      [`nyanpasu-contracts must not depend on ${dependency}`],
    )
  }
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-contracts',
      'nyanpasu-contracts v0.1.0\nnyanpasu-config v0.1.0\nnyanpasu-paths v0.1.0\nserde v1.0\n',
    ),
    [],
  )
})

test('contracts rejects the HTTP dependency introduced by the IPC client', () => {
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-contracts',
      'nyanpasu-contracts v0.1.0\nnyanpasu-ipc v0.1.0\nreqwest v0.13.0\n',
    ),
    ['nyanpasu-contracts must not depend on reqwest'],
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

test('implementations reject core dependencies', () => {
  assert.deepStrictEqual(
    dependencyViolations('nyanpasu-scripts', 'nyanpasu-core v0.1.0\n'),
    ['nyanpasu-scripts must not depend on nyanpasu-core'],
  )
  assert.deepStrictEqual(
    dependencyViolations(
      'nyanpasu-scripts',
      'nyanpasu-scripts v0.1.0\nnyanpasu-config v0.1.0\nnyanpasu-contracts v0.1.0\n',
    ),
    [],
  )
})

test('foundation, core and diagnose reject implementation dependencies', () => {
  for (const owner of [
    'nyanpasu-config',
    'nyanpasu-contracts',
    'nyanpasu-persistence',
    'nyanpasu-profile-transform',
    'nyanpasu-core',
    'nyanpasu-diagnose',
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

for (const dependency of [
  'redb',
  'nyanpasu-core-manager',
  'nyanpasu-contracts',
  'nyanpasu-profile-transform',
]) {
  test(`persistence rejects ${dependency} and accepts IO libraries`, () => {
    assert.deepStrictEqual(
      dependencyViolations('nyanpasu-persistence', `${dependency} v0.1.0\n`),
      [`nyanpasu-persistence must not depend on ${dependency}`],
    )
    assert.deepStrictEqual(
      dependencyViolations(
        'nyanpasu-persistence',
        'nyanpasu-utils v0.1.1\nbackon v1.6\natomicwrites v0.4.4\n',
      ),
      [],
    )
  })
}

for (const owner of ['nyanpasu-config', 'nyanpasu-contracts']) {
  test(`${owner} rejects persistence and accepts lower-level libraries`, () => {
    assert.deepStrictEqual(
      dependencyViolations(owner, 'nyanpasu-persistence v0.1.0\n'),
      [`${owner} must not depend on nyanpasu-persistence`],
    )
    assert.deepStrictEqual(dependencyViolations(owner, 'serde v1.0\n'), [])
  })
}

for (const owner of [
  'nyanpasu-config',
  'nyanpasu-contracts',
  'nyanpasu-persistence',
  'nyanpasu-profile-transform',
  'nyanpasu-core',
  'nyanpasu-scripts',
  'nyanpasu-diagnose',
]) {
  test(`${owner} rejects profile IO and accepts lower-level libraries`, () => {
    assert.deepStrictEqual(
      dependencyViolations(owner, 'nyanpasu-profile-io v0.1.0\n'),
      [`${owner} must not depend on nyanpasu-profile-io`],
    )
    assert.deepStrictEqual(dependencyViolations(owner, 'serde v1.0\n'), [])
  })
}

for (const dependency of [
  'nyanpasu-core',
  'nyanpasu-scripts',
  'nyanpasu-net',
]) {
  test(`profile IO rejects ${dependency} and accepts foundation packages`, () => {
    assert.deepStrictEqual(
      dependencyViolations('nyanpasu-profile-io', `${dependency} v0.1.0\n`),
      [`nyanpasu-profile-io must not depend on ${dependency}`],
    )
    assert.deepStrictEqual(
      dependencyViolations(
        'nyanpasu-profile-io',
        'nyanpasu-profile-io v0.1.0\nnyanpasu-config v0.1.0\nnyanpasu-contracts v0.1.0\nnyanpasu-persistence v0.1.0\n',
      ),
      [],
    )
  })
}

for (const dependency of [
  'nyanpasu-core',
  'nyanpasu-profile-io',
  'nyanpasu-scripts',
]) {
  test(`net rejects ${dependency} and accepts foundation packages and HTTP libraries`, () => {
    assert.deepStrictEqual(
      dependencyViolations('nyanpasu-net', `${dependency} v0.1.0\n`),
      [`nyanpasu-net must not depend on ${dependency}`],
    )
    assert.deepStrictEqual(
      dependencyViolations(
        'nyanpasu-net',
        'nyanpasu-net v0.1.0\nnyanpasu-config v0.1.0\nnyanpasu-contracts v0.1.0\nreqwest v0.13.0\n',
      ),
      [],
    )
  })
}

for (const owner of [
  'nyanpasu-config',
  'nyanpasu-contracts',
  'nyanpasu-persistence',
  'nyanpasu-profile-transform',
  'nyanpasu-core',
  'nyanpasu-scripts',
  'nyanpasu-diagnose',
]) {
  test(`${owner} rejects net and accepts lower-level libraries`, () => {
    assert.deepStrictEqual(
      dependencyViolations(owner, 'nyanpasu-net v0.1.0\n'),
      [`${owner} must not depend on nyanpasu-net`],
    )
    assert.deepStrictEqual(dependencyViolations(owner, 'serde v1.0\n'), [])
  })
}
