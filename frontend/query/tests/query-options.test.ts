import { expect, test } from 'vitest'
import { RpcFailure } from '@nyanpasu/rpc'
import {
  invokeMutation,
  MutationUnconfirmedError,
  shouldRetryRpcQuery,
} from '../src/ipc/query-options'

test.each(['application_error', 'invalid_params'])(
  'a reported %s is confirmed and preserves the RPC failure',
  async (kind) => {
    const failure = new RpcFailure('patch_app_config', {
      kind,
      message: 'Refused',
      operation_id: 'operation-1',
    })

    await expect(
      invokeMutation(
        {
          mutationFn: async () => {
            throw failure
          },
        },
        undefined,
      ),
    ).rejects.toBe(failure)
  },
)

test('a lost mutation reply remains unconfirmed with its original cause', async () => {
  const cause = new Error('connection lost')
  const failure: unknown = await invokeMutation(
    {
      mutationFn: async () => {
        throw cause
      },
    },
    undefined,
  ).catch((error) => error)

  expect(failure).toBeInstanceOf(MutationUnconfirmedError)
  expect(failure).toHaveProperty('cause', cause)
})

test.each([
  'unauthorized',
  'forbidden',
  'unsupported',
  'unknown_method',
  'invalid_params',
])('shouldRetryRpcQuery stops immediately on %s failures', (kind) => {
  const failure = new RpcFailure('get_proxies', {
    kind,
    message: 'Refused',
  })

  expect(shouldRetryRpcQuery(0, failure)).toBe(false)
})

test('shouldRetryRpcQuery allows bounded retries for transient failures', () => {
  const failure = new RpcFailure('get_proxies', {
    kind: 'application_error',
    message: 'Temporary failure',
  })

  expect(shouldRetryRpcQuery(0, failure)).toBe(true)
  expect(shouldRetryRpcQuery(2, failure)).toBe(true)
  expect(shouldRetryRpcQuery(3, failure)).toBe(false)
  expect(shouldRetryRpcQuery(0, new Error('network hiccup'))).toBe(true)
  expect(shouldRetryRpcQuery(3, new Error('network hiccup'))).toBe(false)
})
