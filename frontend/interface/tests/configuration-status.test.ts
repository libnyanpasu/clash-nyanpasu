import { describe, expect, it } from 'vitest'
import type { ConfigurationStatus, SourceStatus } from '../src/ipc/bindings'
import {
  acceptConfigurationStatus,
  attentionSources,
  sourceMessage,
} from '../src/ipc/configuration-status'
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
  sources: [],
  recent_operations: [],
  active: null,
  queued: [],
})
const source = (
  profile: string,
  outcome: SourceStatus['outcome'],
  health: SourceStatus['health'],
): SourceStatus => ({
  profile,
  name: profile.toUpperCase(),
  origin: 'scheduled_refresh',
  outcome,
  health,
  at: 0,
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
  it('shows only the background sources that need attention', () => {
    const failed = source(
      'failed',
      { kind: 'failed', message: 'download failed' },
      'blocked',
    )
    const rejected = source(
      'rejected',
      {
        kind: 'rejected',
        code: 'external_source_rejected',
        message: 'not a mapping',
      },
      'blocked',
    )
    const withSources = {
      ...status(1, 'healthy'),
      sources: [
        source(
          'committed',
          { kind: 'committed', operation_id: 'op' },
          'healthy',
        ),
        failed,
        source(
          'superseded',
          { kind: 'superseded', reason: 'changed' },
          'healthy',
        ),
        rejected,
      ],
    }
    expect(attentionSources(withSources)).toEqual([failed, rejected])
    expect(attentionSources(status(1, 'healthy'))).toEqual([])
    expect(attentionSources(withSources).map(sourceMessage)).toEqual([
      'download failed',
      'not a mapping',
    ])
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
