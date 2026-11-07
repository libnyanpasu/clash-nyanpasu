import assert from 'node:assert/strict'
import { test } from 'node:test'
import { coreModuleViolations } from './core-module-boundaries.ts'

function check(
  consumer: string,
  source: string,
  providers: Record<string, string> = {},
) {
  const files: Record<string, string> = {
    'lib.rs': 'mod actor_v2; mod state; mod client; mod service;',
    'actor_v2.rs': '',
    'state.rs': '',
    'service.rs': 'pub struct Bridge;',
    'client.rs': `pub struct NyanpasuClient;
      pub mod application_workflow { pub struct ApplicationWorkflowClient; pub struct Intent; }
      pub mod effects { pub struct CommitNotifications; pub struct EffectKind; }
      pub mod jobs { pub struct Job; }
      pub mod validation { pub struct Contract; }`,
    ...providers,
    [consumer]: source,
  }
  return coreModuleViolations((file) => files[file])
}

test('application_may_import_control', () => {
  assert.deepEqual(
    check('client.rs', 'use crate::actor_v2::Control;', {
      'actor_v2.rs': 'pub struct Control;',
    }),
    [],
  )
})

test('control_cannot_import_client', () => {
  for (const module of ['actor_v2', 'clash', 'proxies', 'connections']) {
    assert.match(
      check(`${module}.rs`, 'use crate::client::NyanpasuClient;', {
        'lib.rs': `mod ${module}; mod client;`,
      }).join('\n'),
      /forbidden control/,
    )
  }
  for (const source of [
    'use crate::client::NyanpasuClient;',
    'use crate::{client as app}; fn f() { app::NyanpasuClient; }',
    'fn f(_: crate::client::NyanpasuClient) {}',
  ]) {
    assert.match(
      check('actor_v2.rs', source).join('\n'),
      /forbidden control import client/,
    )
  }
})

test('source_owners_cannot_import_workflow', () => {
  for (const name of ['ApplicationWorkflowClient', 'Intent']) {
    assert.match(
      check(
        'state.rs',
        `use crate::client::application_workflow::${name};`,
      ).join('\n'),
      /forbidden state import/,
    )
  }
})

test('source_owners_may_consume_effects_and_value_contracts', () => {
  assert.deepEqual(
    check(
      'state.rs',
      `use crate::client::{effects::{CommitNotifications, EffectKind}, jobs::Job, validation::Contract};
    use nyanpasu_runtime_values::runtime::MutationOutcome;`,
    ),
    [],
  )
})

test('module_policy_resolves_grouped_relative_and_aliased_paths', () => {
  assert.match(
    check(
      'actor_v2.rs',
      `mod nested { use super::super::{client::{self as app, NyanpasuClient as Facade}}; }`,
    ).join('\n'),
    /forbidden control/,
  )
  assert.deepEqual(
    check(
      'state.rs',
      `mod nested { use super::super::service::{self as bridge, Bridge}; fn f() { bridge::Bridge; } }`,
    ),
    [],
  )
  assert.deepEqual(
    check(
      'state.rs',
      `fn a() { use crate::service as other; other::Bridge; } fn b() { use nyanpasu_ipc as other; other::client::Client; }`,
    ),
    [],
  )
  assert.match(
    check(
      'state.rs',
      `fn a() { use crate::client::application_workflow as other; other::Intent; } fn b() { use crate::service as other; other::Bridge; }`,
    ).join('\n'),
    /forbidden state/,
  )
  assert.deepEqual(
    check(
      'state.rs',
      `mod one { use crate::service as other; } mod two { use nyanpasu_ipc as other; other::client::Client; }`,
    ),
    [],
  )
  assert.match(
    check(
      'state.rs',
      `use crate::client as app; fn f() { { use crate::service as app; app::Bridge; } app::application_workflow::Intent; }`,
    ).join('\n'),
    /forbidden state/,
  )
  assert.deepEqual(
    check(
      'state.rs',
      `use self::nested::Bridge; mod nested { pub use crate::service::Bridge; }`,
    ),
    [],
  )
})

test('module_policy_checks_tests_and_ignores_comments_and_strings', () => {
  assert.deepEqual(
    check(
      'actor_v2.rs',
      `// use crate::client::NyanpasuClient;
    /* nested /* use crate::client::*; */ comment */
    fn f() { let s = r#"use crate::client::NyanpasuClient;"#; let c = '{';
      let multiline = "first line
        use crate::client::NyanpasuClient;
      last line";
    }`,
    ),
    [],
  )
  assert.match(
    check(
      'actor_v2.rs',
      `#[cfg(test)] mod tests { use crate::client::NyanpasuClient; }`,
    ).join('\n'),
    /actor_v2.rs:1: forbidden/,
  )
  assert.match(
    check('actor_v2.rs', `mod tests;`, {
      'actor_v2/tests.rs': 'use crate::client::NyanpasuClient;',
    }).join('\n'),
    /actor_v2\/tests.rs:1: forbidden/,
  )
})

test('module_policy_allows_self_and_super_globs_within_policed_modules', () => {
  for (const module of ['actor_v2', 'state']) {
    assert.deepEqual(
      check(`${module}.rs`, `use self::*; mod tests { use super::*; }`),
      [],
    )
    assert.match(
      check(
        `${module}.rs`,
        `use crate::client::application_workflow::ApplicationWorkflowClient;
        mod tests { use super::*; }`,
      ).join('\n'),
      new RegExp(`${module}.rs:1: forbidden`),
    )
  }
})

test('module_policy_allows_service_bridge_and_external_ipc_client', () => {
  assert.deepEqual(
    check(
      'actor_v2.rs',
      `use crate::service::Bridge; use nyanpasu_ipc::client::Client; use ::nyanpasu_ipc::client as ipc;
    fn f() { ipc::Client; }`,
      {
        'service.rs':
          'pub struct Bridge; fn implementation() { crate::client::NyanpasuClient; }',
      },
    ),
    [],
  )
})

test('module_policy_resolves_aliases_inherited_through_globs', () => {
  for (const prefix of ['', 'self::', 'super::']) {
    assert.match(
      check(
        'state.rs',
        `use crate::client as app;
        mod tests { use super::*;
          mod nested { use super::*; use ${prefix}app::application_workflow::ApplicationWorkflowClient; }
        }`,
      ).join('\n'),
      /forbidden state import client::application_workflow/,
    )
  }
  for (const glob of ['super::*', 'self::*']) {
    assert.match(
      check(
        'state.rs',
        `use crate::client as app;
        mod tests { use super::*;
          ${glob === 'super::*' ? 'mod nested' : 'fn f()'} { use ${glob}; use app::application_workflow::ApplicationWorkflowClient; }
        }`,
      ).join('\n'),
      /forbidden state import client::application_workflow/,
    )
    assert.match(
      check(
        'actor_v2.rs',
        `use crate::service as provider;
        mod tests { use super::*;
          ${glob === 'super::*' ? 'mod nested' : 'fn f()'} { use ${glob}; use provider::Bridge; }
        }`,
        {
          'service.rs': 'pub use crate::client::NyanpasuClient as Bridge;',
        },
      ).join('\n'),
      /forbidden control import client/,
    )
  }
  for (const module of ['actor_v2', 'state']) {
    assert.deepEqual(
      check(
        `${module}.rs`,
        `use crate::service as provider;
        mod tests { use super::*;
          mod nested { use super::*; use self::*; use provider::Bridge; }
        }`,
      ),
      [],
    )
  }
})

test('module_policy_rejects_raw_identifiers_anywhere_in_paths', () => {
  assert.deepEqual(
    check(
      'actor_v2.rs',
      'struct Value { pub r#type: String } fn f() { Value { r#type: String::new() }; }',
    ),
    [],
  )
  for (const source of [
    'use crate::client::r#application_workflow::ApplicationWorkflowClient;',
    'fn f(_: crate::client::r#application_workflow::ApplicationWorkflowClient) {}',
    'use crate::service as provider; use provider::r#Bridge;',
    'use crate::service as provider; fn f(_: provider::r#Bridge) {}',
    'use crate::service::{r#Bridge};',
    'use crate::service as r#provider;',
  ]) {
    assert.match(
      check('state.rs', source).join('\n'),
      /state.rs:1: unsupported raw identifier path; use explicit imports/,
      source,
    )
  }
  assert.match(
    check('state.rs', 'use crate::service::Bridge;', {
      'service.rs':
        'pub use crate::client::r#ApplicationWorkflowClient as Bridge;',
    }).join('\n'),
    /service.rs:1: unsupported raw identifier path; use explicit imports/,
  )
})

test('module_policy_rejects_indirect_forbidden_reexports', () => {
  for (const consumer of ['actor_v2.rs', 'state.rs']) {
    assert.match(
      check(consumer, 'use crate::service::Bridge;', {
        'service.rs':
          'pub(crate) use crate::client::application_workflow::ApplicationWorkflowClient as Bridge;',
      }).join('\n'),
      /forbidden/,
    )
    assert.match(
      check(consumer, 'use crate::Bridge;', {
        'lib.rs':
          'mod actor_v2; mod state; mod client; mod service; pub use service::Bridge;',
        'service.rs':
          'pub use crate::client::application_workflow::Intent as Bridge;',
      }).join('\n'),
      /forbidden/,
    )
  }
  for (const hops of [8, 9]) {
    const chain = Array.from(
      { length: hops },
      (_, i) =>
        `pub use ${i + 1 === hops ? 'crate::service::Bridge' : `self::Alias${i + 1}`} as Alias${i};`,
    ).join('\n')
    const result = check('state.rs', 'fn f(_: crate::Alias0) {}', {
      'lib.rs': `mod state; mod client; mod service; ${chain}`,
    })
    if (hops === 8) assert.deepEqual(result, [])
    else assert.match(result.join('\n'), /exceeds 8 hops/)
    const forbidden = check('state.rs', 'fn f(_: crate::Alias0) {}', {
      'lib.rs': `mod state; mod client; ${chain.replace('crate::service::Bridge', 'crate::client::application_workflow::Intent')}`,
    })
    assert.match(
      forbidden.join('\n'),
      hops === 8 ? /forbidden state/ : /exceeds 8 hops/,
    )
  }
  assert.match(
    check('state.rs', 'use crate::Alias0;', {
      'lib.rs':
        'mod state; mod client; pub use self::Alias1 as Alias0; pub use client::application_workflow::Intent as Alias1;',
    }).join('\n'),
    /forbidden state/,
  )
})

test('module_policy_rejects_globs_cycles_and_unsupported_forms', () => {
  assert.match(
    coreModuleViolations(() => undefined).join('\n'),
    /cannot locate core lib.rs/,
  )
  for (const name of [
    'assert',
    'assert_eq',
    'assert_ne',
    'debug_assert',
    'debug_assert_eq',
    'format',
    'matches',
    'panic',
    'println',
    'unimplemented',
    'unreachable',
    'vec',
    'write',
    'include_str',
    'include_bytes',
    'anyhow::anyhow',
    'anyhow::bail',
    'anyhow::ensure',
    'nanoid::nanoid',
    'serde_json::json',
    'snafu::ensure',
    'tokio::select',
    'tokio::try_join',
    'tracing::debug',
    'tracing::error',
    'tracing::info',
    'tracing::warn',
  ]) {
    assert.deepEqual(check('actor_v2.rs', `fn f() { ${name}!(); }`), [], name)
    assert.match(
      check(
        'actor_v2.rs',
        `fn f() { ${name}!({ use crate::client::NyanpasuClient; }); }`,
      ).join('\n'),
      /unsupported macro-generated/,
    )
  }
  assert.deepEqual(
    check(
      'actor_v2.rs',
      'use serde_json::json as value; fn f() { value!({}); }',
    ),
    [],
  )
  assert.deepEqual(
    check(
      'actor_v2.rs',
      'fn f() { macro_rules! local { () => { true } } local!(); }',
    ),
    [],
  )
  for (const [source, diagnostic] of [
    ['use crate::service::*;', /glob/],
    ['use self::nested::*;', /glob/],
    ['use super::*;', /glob/],
    ['mod tests { use super::super::*; }', /glob/],
    ['use crate::missing::Thing;', /cannot locate/],
    ['use crate::service as a; use nyanpasu_ipc as a; a::Bridge;', /ambiguous/],
    ['#[path = "elsewhere.rs"] mod other;', /unsupported.*path/],
    ['extern crate nyanpasu_core as hidden;', /extern crate/],
    [
      'macro_rules! imports { () => { use crate::client::NyanpasuClient; } }',
      /macro-generated/,
    ],
    ['include!("imports.rs");', /macro-generated/],
    ['make_imports!();', /macro-generated/],
    ['generator::make_imports!();', /macro-generated/],
    ['fn f() { generator::imports!(); }', /unsupported macro expansion/],
    ['fn f() { vec![{ imports!(); }]; }', /unsupported macro expansion/],
    [
      'fn f() { generator::imports! { use crate::service::Bridge; } }',
      /macro-generated/,
    ],
    ['use crate::r#client::NyanpasuClient;', /unsupported/],
    ['fn f() { <T as Trait>::thing(); }', /unsupported qualified/],
  ] as const) {
    const result = check('actor_v2.rs', source).join('\n')
    assert.match(result, diagnostic, source)
    assert.match(result, /actor_v2.rs:\d+:.*use explicit imports/)
  }
  assert.match(
    check('state.rs', 'use crate::service::Bridge;', {
      'service.rs':
        'pub use self::Other as Bridge; pub use self::Bridge as Other;',
    }).join('\n'),
    /cyclic/,
  )
  assert.match(
    check('state.rs', 'use crate::service::Bridge;', {
      'service.rs': 'pub use crate::client::application_workflow::*;',
    }).join('\n'),
    /glob/,
  )
  assert.match(
    check('state.rs', 'use crate::service::Bridge;', {
      'service.rs':
        'pub type Bridge = crate::client::application_workflow::Intent;',
    }).join('\n'),
    /unsupported type alias/,
  )
  assert.match(
    check('state.rs', 'use crate::service::Bridge;', {
      'service.rs': 'pub type Bridge<T> = <T as Trait>::Value;',
    }).join('\n'),
    /unsupported qualified/,
  )
  assert.match(
    check('state.rs', 'use hidden::client::application_workflow::Intent;', {
      'lib.rs': 'extern crate self as hidden; mod state; mod client;',
    }).join('\n'),
    /unsupported extern crate/,
  )
  assert.match(
    check('state.rs', 'use crate::service::Bridge;', {
      'service.rs':
        '#[cfg(a)] pub use crate::client::application_workflow::Intent as Bridge; #[cfg(b)] pub struct Bridge;',
    }).join('\n'),
    /ambiguous/,
  )
  assert.deepEqual(
    check('state.rs', 'use crate::service::Bridge;', {
      'service.rs':
        'pub struct Bridge; #[cfg(test)] mod tests { use super::*; }',
    }),
    [],
  )
})
