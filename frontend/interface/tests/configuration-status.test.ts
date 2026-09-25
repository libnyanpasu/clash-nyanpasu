import { describe, expect, it } from 'vitest'
import type { ConfigurationStatus } from '../src/ipc/bindings'
import { acceptConfigurationStatus } from '../src/ipc/configuration-status'
import {
  invokeMutation,
  MutationUnconfirmedError,
} from '../src/ipc/query-options'

const status = (
  event_seq: number,
  health: ConfigurationStatus['runtime']['health'],
): ConfigurationStatus => ({
  event_seq,
  maintenance: null,
  runtime: {
    health,
    operation_id: null,
    attempts: 0,
    automatic_remaining: 0,
    message: null,
  },
  source_versions: { application: 1, clash: 1, session: 1, profiles: 1 },
  effects: [],
  recent_operations: [],
  active: null,
  queued: [],
})
describe('configuration status', () => {
  it('keeps a newer failure when an old success arrives late', () => {
    const blocked = status(12, 'blocked')
    expect(acceptConfigurationStatus(blocked, status(11, 'healthy'))).toBe(
      blocked,
    )
    expect(acceptConfigurationStatus(blocked, status(12, 'healthy'))).toBe(
      blocked,
    )
    expect(
      acceptConfigurationStatus(blocked, status(13, 'healthy')).runtime.health,
    ).toBe('healthy')
  })
  it('distinguishes a lost reply from a confirmed domain refusal', async () => {
    await expect(
      invokeMutation(
        {
          mutationFn: async () => {
            throw new Error('transport disconnected')
          },
        },
        [],
      ),
    ).rejects.toBeInstanceOf(MutationUnconfirmedError)
    const bug = new TypeError('undefined is not a function')
    await expect(
      invokeMutation(
        {
          mutationFn: async () => {
            throw bug
          },
        },
        [],
      ),
    ).rejects.toBe(bug)
    const refused = { status: 'error', error: 'validation rejected' }
    expect(await invokeMutation({ mutationFn: async () => refused }, [])).toBe(
      refused,
    )
  })
})
