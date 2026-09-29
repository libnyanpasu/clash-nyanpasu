import { expect, test } from 'vitest'
import { m } from '@/paraglide/messages'
import { ipcErrorMessage } from '@/utils/ipc-error'
import type { IpcError, ProfilesError } from '@nyanpasu/interface'

const profiles = (error: ProfilesError): IpcError => ({
  kind: { domain: 'profiles', error },
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
      reasons: ['port 7890 is in use', 'second'],
      runtime: { kind: 'untouched' },
    }),
  ).toBe(m.error_commit_runtime_refused({ reason: 'port 7890 is in use' }))
  expect(
    commit({
      kind: 'runtime_failed',
      reasons: [],
      runtime: { kind: 'unknown', detail: 'no answer' },
    }),
  ).toBe(
    `${m.error_commit_runtime_failed({ reason: '' })} (${m.error_commit_runtime_unknown()})`,
  )
  expect(commit({ kind: 'validate_state' })).toBe(
    m.error_commit_validate_state(),
  )
})
