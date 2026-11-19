import { describe, expect, it } from 'vitest'
import { createIconTransport, createRpcClient, type RpcTransport } from '../src'
import type {
  AppUpdateSnapshot,
  AppUpdateSummary,
  ClashCore_Deserialize,
  ClashCore_Serialize,
  FolderId,
  ProfileId,
  PublicDegradation_Serialize,
  RpcAppUpdateOutcome,
  RpcFolderMutationOutcome_Serialize,
  RpcProfileMutationOutcome_Serialize,
  RpcUnitMutationOutcome_Serialize,
} from '../src/types'

type IsAny<T> = 0 extends 1 & T ? true : false
type AssertFalse<T extends false> = T

const _notAnyUnit: AssertFalse<IsAny<RpcUnitMutationOutcome_Serialize>> = false
const _notAnyProfile: AssertFalse<IsAny<RpcProfileMutationOutcome_Serialize>> =
  false
const _notAnyFolder: AssertFalse<IsAny<RpcFolderMutationOutcome_Serialize>> =
  false
const _notAnyCore: AssertFalse<IsAny<ClashCore_Serialize>> = false
const _notAnyAppUpdate: AssertFalse<IsAny<RpcAppUpdateOutcome>> = false

function makeContractClient(invokeImpl: <T>(method: string) => Promise<T>) {
  const transport: RpcTransport = {
    icons: createIconTransport(false),
    commands: { invoke: invokeImpl },
    events: {
      listen: async () => () => {},
      once: async () => () => {},
      emit: async () => {},
      listenResync: () => () => {},
      dispose: () => {},
    },
  }
  return createRpcClient(transport)
}

describe('RPC generated TypeScript contracts', () => {
  it('enforces concrete unit, ProfileId, and FolderId mutation outcomes without unbound generics', async () => {
    const publicDegradation: PublicDegradation_Serialize = {
      phase: 'runtime_apply',
      reason: {
        code: 'runtime_deferred',
        cause: { kind: 'owner_unresponsive' },
      },
      retryable: true,
    }

    const client = makeContractClient(async <T>(method: string) => {
      if (method === 'create_profile') {
        const outcome: RpcProfileMutationOutcome_Serialize = {
          status: 'committed',
          commits: [],
          notifications_pending: false,
          value: 'profile-1' as ProfileId,
        }
        return outcome as T
      }
      if (method === 'create_profile_folder') {
        const outcome: RpcFolderMutationOutcome_Serialize = {
          status: 'committed_degraded',
          commits: [],
          notifications_pending: true,
          value: 'folder-1' as FolderId,
          degradations: [publicDegradation],
        }
        return outcome as T
      }
      const outcome: RpcUnitMutationOutcome_Serialize = {
        status: 'committed_degraded',
        commits: [],
        notifications_pending: false,
        value: null,
        degradations: [publicDegradation],
      }
      return outcome as T
    })

    const unitRes: RpcUnitMutationOutcome_Serialize =
      await client.setReleaseChannel('beta')
    const profileRes: RpcProfileMutationOutcome_Serialize =
      await client.createProfile(
        {
          metadata: { name: 'Demo' },
          definition: {
            type: 'config',
            config: { type: 'composition', base: null },
          },
        },
        null,
      )
    const folderRes: RpcFolderMutationOutcome_Serialize =
      await client.createProfileFolder('Work', [])

    expect(unitRes.status).toBe('committed_degraded')
    expect(profileRes.status).toBe('committed')
    expect(folderRes.status).toBe('committed_degraded')
    expect([
      _notAnyUnit,
      _notAnyProfile,
      _notAnyFolder,
      _notAnyCore,
      _notAnyAppUpdate,
    ]).not.toContain(true)
  })

  it('rejects mismatched concrete outcome shapes and non-canonical ClashCore_Serialize values at compile time', () => {
    const acceptUnitOutcome = (_outcome: RpcUnitMutationOutcome_Serialize) => {}
    const acceptProfileOutcome = (
      _outcome: RpcProfileMutationOutcome_Serialize,
    ) => {}
    const acceptFolderOutcome = (
      _outcome: RpcFolderMutationOutcome_Serialize,
    ) => {}
    const acceptCanonicalCore = (_core: ClashCore_Serialize) => {}
    const acceptDeserializeCore = (_core: ClashCore_Deserialize) => {}
    const acceptPublicDegradation = (_item: PublicDegradation_Serialize) => {}
    const acceptAppUpdateOutcome = (_outcome: RpcAppUpdateOutcome) => {}

    acceptCanonicalCore('mihomo')
    acceptDeserializeCore('clash-meta')

    // @ts-expect-error ClashCore_Serialize must not accept deserialize-only aliases or arbitrary strings.
    acceptCanonicalCore('clash-meta')
    // @ts-expect-error ClashCore_Serialize must not accept unknown strings.
    acceptCanonicalCore('bogus-core')

    acceptUnitOutcome({
      status: 'committed',
      commits: [],
      notifications_pending: false,
      value: null,
    })
    acceptUnitOutcome({
      status: 'committed',
      commits: [],
      notifications_pending: false,
      // @ts-expect-error unit mutation outcome value must not accept a non-null string.
      value: 'unexpected-profile-id',
    })

    acceptProfileOutcome({
      status: 'committed',
      commits: [],
      notifications_pending: false,
      value: 'p-1',
    })
    acceptProfileOutcome({
      status: 'committed',
      commits: [],
      notifications_pending: false,
      // @ts-expect-error profile mutation outcome value must not accept null.
      value: null,
    })

    acceptFolderOutcome({
      status: 'committed',
      commits: [],
      notifications_pending: false,
      value: 'f-1',
    })
    acceptFolderOutcome({
      status: 'committed',
      commits: [],
      notifications_pending: false,
      // @ts-expect-error folder mutation outcome value must not accept numbers.
      value: 42,
    })

    acceptPublicDegradation({
      phase: 'runtime_apply',
      reason: { code: 'http_server_unavailable' },
      retryable: false,
      // @ts-expect-error PublicDegradation_Serialize omits internal raw host diagnostic message.
      message: 'raw internal host cause',
    })

    const fullSnapshot: AppUpdateSnapshot = {
      revision: 1,
      phase: 'idle',
      release: null,
      downloaded: 0,
      total: null,
      speed: null,
      source: null,
      error: null,
      last_checked_at: null,
      supported: true,
      endpoints: [],
    }
    const safeSummary: AppUpdateSummary = {
      revision: 1,
      phase: 'idle',
      release: null,
      downloaded: 0,
      total: null,
      speed: null,
      source: 'github',
      has_error: false,
      last_checked_at: null,
      supported: true,
    }
    acceptAppUpdateOutcome(fullSnapshot)
    acceptAppUpdateOutcome(safeSummary)
  })
})
