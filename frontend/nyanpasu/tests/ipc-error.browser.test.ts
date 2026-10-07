import { expect, test } from 'vitest'
import { m } from '@/paraglide/messages'
import {
  degradationReasonMessage,
  effectFailureMessage,
  ipcErrorMessage,
} from '@/utils/ipc-error'
import type {
  ConfigError,
  EffectsError,
  IpcError,
  OsProxyError,
  ProfilesError,
  RuntimeError,
  StorageOperationError,
  SystemDnsError,
} from '@nyanpasu/rpc/types'

const profiles = (error: ProfilesError): IpcError => ({
  kind: { domain: 'profiles', error },
  message: 'the backend text',
  detail: 'the backend text: caused by',
})

const runtime = (error: RuntimeError): IpcError => ({
  kind: { domain: 'runtime', error },
  message: 'the backend text',
  detail: 'the backend text: caused by',
})

const config = (error: ConfigError): IpcError => ({
  kind: { domain: 'config', error },
  message: 'the backend text',
  detail: 'the backend text: caused by',
})

const storage = (error: StorageOperationError): IpcError => ({
  kind: { domain: 'storage', error },
  message: 'the backend text',
  detail: 'the backend text: caused by',
})

const systemDns = (error: SystemDnsError): IpcError => ({
  kind: { domain: 'system_dns', error },
  message: 'the backend text',
  detail: 'the backend text: caused by',
})

const systemProxy = (error: OsProxyError): IpcError => ({
  kind: { domain: 'system_proxy', error },
  message: 'the backend text',
  detail: 'the backend text: caused by',
})

const effects = (error: EffectsError): IpcError => ({
  kind: { domain: 'effects', error },
  message: 'the backend text',
  detail: 'the backend text: caused by',
})

const uid = 'p1'

test('an unclassified error shows the backend message', () => {
  expect(
    ipcErrorMessage({
      kind: { domain: 'unknown' },
      message: 'the backend text',
      detail: '',
    }),
  ).toBe('the backend text')
})

test('a profiles error is localized with the context it carries', () => {
  expect(ipcErrorMessage(profiles({ kind: 'profile_not_found', uid }))).toBe(
    m.error_profiles_profile_not_found(),
  )
  expect(
    ipcErrorMessage(
      profiles({
        kind: 'profile_in_use',
        uid,
        referrers: ['a', 'b'],
        current: false,
        global_transforms: false,
      }),
    ),
  ).toBe(m.error_profiles_profile_in_use_by_profiles({ count: 2 }))
  expect(
    ipcErrorMessage(
      profiles({
        kind: 'profile_in_use',
        uid,
        referrers: [],
        current: true,
        global_transforms: false,
      }),
    ),
  ).toBe(m.error_profiles_profile_in_use_by_selection())
})

test('a failed download names the host and the status but not the token', () => {
  const url = 'https://sub.example.com:8443/api/v1?token=secret'
  const status = ipcErrorMessage(
    profiles({
      kind: 'fetch_subscription',
      url,
      source: { kind: 'subscription_http_status', status: 404 },
    }),
  )
  expect(status).toBe(
    m.error_profiles_fetch_subscription_status({
      host: 'sub.example.com:8443',
      status: 404,
    }),
  )
  expect(status).not.toContain('secret')

  expect(
    ipcErrorMessage(
      profiles({
        kind: 'fetch_subscription',
        url,
        source: { kind: 'request_subscription' },
      }),
    ),
  ).toBe(
    m.error_profiles_fetch_subscription_failed({
      host: 'sub.example.com:8443',
    }),
  )
})

test('a rejected download says what is wrong with the content', () => {
  expect(
    ipcErrorMessage(
      profiles({
        kind: 'profile_content_rejected',
        source: { kind: 'missing_proxies' },
      }),
    ),
  ).toBe(m.error_profiles_content_missing_proxies())
})

test('a storage failure tells an unsafe path from an unreadable file', () => {
  expect(
    ipcErrorMessage(
      profiles({
        kind: 'materialization',
        operation: 'prepare_file_first',
        source: { kind: 'reserved_path', path: '.profile-materialization-v1' },
      }),
    ),
  ).toBe(m.error_profiles_storage_unsafe_path())
  expect(
    ipcErrorMessage(
      profiles({
        kind: 'materialization',
        operation: 'prepare_file_first',
        source: { kind: 'write_file', path: 'a.yaml' },
      }),
    ),
  ).toBe(m.error_profiles_storage_failed())
  expect(
    ipcErrorMessage(
      profiles({
        kind: 'read_external_profile',
        target: '/home/u/a.yaml',
        source: { kind: 'read_file', path: '/home/u/a.yaml' },
      }),
    ),
  ).toBe(m.error_profiles_read_external_profile({ path: '/home/u/a.yaml' }))
})

test('an aborted commit says what became of the core', () => {
  const commit = (
    source: Extract<ProfilesError, { kind: 'commit' }>['source'],
  ) => ipcErrorMessage(profiles({ kind: 'commit', source }))

  expect(commit({ kind: 'write_config', runtime: { kind: 'untouched' } })).toBe(
    m.error_commit_write_config(),
  )
  expect(
    commit({ kind: 'write_config', runtime: { kind: 'rolled_back' } }),
  ).toBe(
    `${m.error_commit_write_config()} (${m.error_commit_runtime_rolled_back()})`,
  )
  expect(
    commit({
      kind: 'runtime_refused',
      errors: [
        { kind: 'unsettled_baseline', gap: 'core_transitioning' },
        { kind: 'isolated' },
      ],
      runtime: { kind: 'untouched' },
    }),
  ).toBe(
    m.error_commit_runtime_refused({
      reason: `${m.error_runtime_unsettled_baseline_core_transitioning()}\n\n${m.error_runtime_isolated()}`,
    }),
  )
  expect(
    commit({
      kind: 'runtime_failed',
      errors: [],
      runtime: {
        kind: 'unknown',
        detail: { kind: 'owner_unresponsive', operation_id: 'op1' },
      },
    }),
  ).toBe(
    `${m.error_commit_runtime_failed({ reason: '' })} (${m.error_commit_runtime_unknown()})`,
  )
  expect(commit({ kind: 'validate_state' })).toBe(
    m.error_commit_validate_state(),
  )
})

const coreFailure = {
  kind: null,
  message: 'unreachable',
  retryable: true,
  operation_id: null,
}

test('a core operation names what failed and why the core refused it', () => {
  expect(
    ipcErrorMessage(
      runtime({
        kind: 'apply_runtime',
        failure: {
          kind: 'apply_failed',
          message: 'kept the previous revision',
          retryable: false,
          operation_id: null,
        },
      }),
    ),
  ).toBe(
    `${m.error_runtime_apply_runtime()} (${m.error_runtime_core_reason_apply_failed()})`,
  )
  expect(
    ipcErrorMessage(
      runtime({
        kind: 'stop_service',
        failure: {
          kind: null,
          message: 'service stop failed',
          retryable: true,
          operation_id: null,
        },
      }),
    ),
  ).toBe(m.error_runtime_stop_service())
})

test('unavailable native storage is explained for operations and refused mutations', () => {
  const reason = m.error_runtime_core_reason_native_store_unavailable()
  expect(
    ipcErrorMessage(
      runtime({
        kind: 'apply_runtime',
        failure: {
          kind: 'native_store_unavailable',
          message: 'native storage failed',
          retryable: false,
          operation_id: null,
        },
      }),
    ),
  ).toBe(`${m.error_runtime_apply_runtime()} (${reason})`)
  expect(
    ipcErrorMessage(
      runtime({
        kind: 'core_rejected_config',
        core_kind: 'native_store_unavailable',
        message: 'native storage failed',
      }),
    ),
  ).toBe(`${m.error_runtime_core_rejected_config()} (${reason})`)
})

test('a refused admission is told apart from a failed operation', () => {
  expect(ipcErrorMessage(runtime({ kind: 'isolated' }))).toBe(
    m.error_runtime_isolated(),
  )
  expect(ipcErrorMessage(runtime({ kind: 'shutting_down' }))).toBe(
    m.error_runtime_shutting_down(),
  )
})

test('a refused mutation says what the runtime lacked or rejected', () => {
  expect(
    ipcErrorMessage(
      runtime({ kind: 'unsettled_baseline', gap: 'no_restorable_baseline' }),
    ),
  ).toBe(m.error_runtime_unsettled_baseline_no_restorable_baseline())
  expect(
    ipcErrorMessage(
      runtime({
        kind: 'core_rejected_config',
        core_kind: 'config_check_failed',
        message: 'unknown proxy type',
      }),
    ),
  ).toBe(
    `${m.error_runtime_core_rejected_config()} (${m.error_runtime_core_reason_config_check_failed()})`,
  )
  expect(
    ipcErrorMessage(
      runtime({ kind: 'move_host', host: 'service', failure: coreFailure }),
    ),
  ).toBe(m.error_runtime_move_host({ host: m.error_runtime_host_service() }))
})

test('a failed build names the profile or the transform', () => {
  expect(
    ipcErrorMessage(
      runtime({
        kind: 'build_runtime',
        source: {
          kind: 'build_artifact',
          source: {
            kind: 'run_pipeline',
            source: { kind: 'selected_profile_not_found', profile: 'p1' },
          },
        },
      }),
    ),
  ).toBe(m.error_runtime_build_selected_profile_not_found({ profile: 'p1' }))
  expect(
    ipcErrorMessage(
      runtime({
        kind: 'build_runtime',
        source: {
          kind: 'build_artifact',
          source: {
            kind: 'transforms_failed',
            logs: [],
            failures: [
              { kind: 'profile', id: 't1' },
              { kind: 'builtin', name: 'config_fixer' },
            ],
          },
        },
      }),
    ),
  ).toBe(m.error_runtime_build_transforms_failed({ names: 't1, config_fixer' }))
})

test('runtime preparation failures keep their stage-specific messages', () => {
  expect(
    ipcErrorMessage(
      runtime({
        kind: 'build_runtime',
        source: { kind: 'start_script_runner' },
      }),
    ),
  ).toBe(m.error_runtime_build_start_script_runner())

  expect(
    ipcErrorMessage(
      runtime({
        kind: 'build_runtime',
        source: {
          kind: 'build_artifact',
          source: { kind: 'validate_profiles', errors: [] },
        },
      }),
    ),
  ).toBe(m.error_runtime_build_validate_profiles())

  for (const kind of [
    'serialize_final_config',
    'config_not_mapping',
  ] as const) {
    expect(
      ipcErrorMessage(runtime({ kind: 'build_runtime', source: { kind } })),
    ).toBe(m.error_runtime_render_runtime_config())
  }
})

test('a port in use names the port, and a missing one names the field', () => {
  expect(
    ipcErrorMessage(
      runtime({
        kind: 'resolve_port',
        source: {
          kind: 'resolve_port',
          field: 'mixed',
          source: { kind: 'port_not_available', port: 7890 },
        },
      }),
    ),
  ).toBe(m.error_runtime_port_in_use({ port: 7890 }))
  expect(
    ipcErrorMessage(
      runtime({
        kind: 'resolve_port',
        source: {
          kind: 'resolve_port',
          field: 'external_controller',
          source: { kind: 'no_open_port' },
        },
      }),
    ),
  ).toBe(
    m.error_runtime_no_open_port({
      field: m.error_runtime_port_field_external_controller(),
    }),
  )
})

test('a missing core binary names the core', () => {
  expect(
    ipcErrorMessage(
      runtime({
        kind: 'resolve_core_binary',
        source: { kind: 'find_core_binary', core: 'mihomo' },
      }),
    ),
  ).toBe(m.error_runtime_find_core_binary({ core: 'mihomo' }))
})

test('a degradation localizes its reason, and a runtime cause through the runtime message', () => {
  expect(
    degradationReasonMessage({
      code: 'service_stop_failed',
      cause: { kind: 'shutting_down' },
    }),
  ).toBe(m.error_runtime_shutting_down())
  expect(
    degradationReasonMessage({
      code: 'runtime_deferred',
      cause: { kind: 'isolated' },
    }),
  ).toBe(
    m.mutation_degradation_reason_runtime_deferred({
      reason: m.error_runtime_isolated(),
    }),
  )
  expect(
    degradationReasonMessage({
      code: 'runtime_recovery_required',
      operation_id: 'op1',
      cause: null,
    }),
  ).toBe(m.mutation_degradation_reason_runtime_recovery_required())
  expect(
    degradationReasonMessage({
      code: 'proxy_interruption_failed',
      cause: 'timeout',
    }),
  ).toBe(m.mutation_degradation_reason_interruption_failed())
  expect(
    degradationReasonMessage({
      code: 'profile_auto_activation_failed',
      profile: uid,
      cause: { kind: 'shutting_down' },
    }),
  ).toBe(
    m.mutation_degradation_reason_profile_auto_activation_failed({
      reason: m.error_profiles_shutting_down(),
    }),
  )
})

test('a rejected hotkey list names the shortcut that was wrong', () => {
  const rejected = (
    source: Extract<ConfigError, { kind: 'validate_hotkeys' }>['source'],
  ) => ipcErrorMessage(config({ kind: 'validate_hotkeys', source }))

  expect(rejected({ kind: 'malformed_entry', entry: 'toggle_tun_mode' })).toBe(
    m.error_config_hotkey_malformed_entry({ entry: 'toggle_tun_mode' }),
  )
  expect(rejected({ kind: 'missing_super_key', accelerator: 'Q' })).toBe(
    m.error_config_hotkey_missing_super_key({ accelerator: 'Q' }),
  )
  expect(
    rejected({
      kind: 'duplicate_accelerator',
      accelerator: 'Control+Q',
      first: 'enable_tun_mode',
      second: 'disable_tun_mode',
    }),
  ).toBe(
    m.error_config_hotkey_duplicate_accelerator({
      accelerator: 'Control+Q',
      first: m.settings_nyanpasu_hotkey_enable_tun_mode(),
      second: m.settings_nyanpasu_hotkey_disable_tun_mode(),
    }),
  )
})

test('a configuration change that was not committed says what became of the core', () => {
  expect(
    ipcErrorMessage(
      config({
        kind: 'commit',
        domain: 'clash',
        source: { kind: 'write_config', runtime: { kind: 'rolled_back' } },
      }),
    ),
  ).toBe(
    `${m.error_commit_write_config()} (${m.error_commit_runtime_rolled_back()})`,
  )
  expect(
    ipcErrorMessage(
      config({
        kind: 'version_conflict',
        domain: 'application',
        expected: 1,
        actual: 2,
      }),
    ),
  ).toBe(m.error_config_version_conflict())
  expect(
    ipcErrorMessage(config({ kind: 'leave_nightly_channel', to: 'stable' })),
  ).toBe(m.error_config_leave_nightly_channel())
})

test('a storage failure tells an unopenable database from unreadable data', () => {
  expect(
    ipcErrorMessage(storage({ kind: 'open_database', path: 'storage.redb' })),
  ).toBe(m.error_storage_open())
  expect(
    ipcErrorMessage(storage({ kind: 'read_item', key: 'web:theme' })),
  ).toBe(m.error_storage_access())
  expect(
    ipcErrorMessage(storage({ kind: 'decode_value', key: 'web:theme' })),
  ).toBe(m.error_storage_invalid_value())
})

test('a failed DNS flush names the command and its exit code', () => {
  expect(
    ipcErrorMessage(
      systemDns({ kind: 'flush_rejected', command: 'ipconfig.exe', code: 5 }),
    ),
  ).toBe(
    m.error_system_dns_flush_rejected({ command: 'ipconfig.exe', code: 5 }),
  )
  expect(
    ipcErrorMessage(
      systemDns({
        kind: 'flush_rejected',
        command: 'ipconfig.exe',
        code: null,
      }),
    ),
  ).toBe(
    m.error_system_dns_flush_rejected({ command: 'ipconfig.exe', code: '-' }),
  )
})

test('an effect failure is localized by its code', () => {
  expect(effectFailureMessage('system_proxy_apply_failed')).toBe(
    m.effect_failure_system_proxy_apply_failed(),
  )
  expect(effectFailureMessage('hotkey_partial_registration')).toBe(
    m.effect_failure_hotkey_partial_registration(),
  )
})

test('a failed system proxy write names the address it was going to', () => {
  expect(
    ipcErrorMessage(
      systemProxy({
        kind: 'write_os_proxy',
        enable: true,
        host: '127.0.0.1',
        port: 7890,
      }),
    ),
  ).toBe(m.error_system_proxy_write_os_proxy({ host: '127.0.0.1', port: 7890 }))
  expect(ipcErrorMessage(systemProxy({ kind: 'read_os_proxy' }))).toBe(
    m.error_system_proxy_read_os_proxy(),
  )
})

test('a retry that cannot reach the effects owner says it is not running', () => {
  expect(ipcErrorMessage(effects({ kind: 'effects_stopped' }))).toBe(
    m.error_effects_stopped(),
  )
})
