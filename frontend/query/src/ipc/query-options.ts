import { RpcFailure, unwrapResult, type Result } from '@nyanpasu/rpc'
import type { QueryKey } from '@tanstack/react-query'

export function unwrapQueryOptions<
  TData,
  TError,
  TOptions extends { queryKey: QueryKey },
>(
  options: TOptions,
  queryFn: (
    context: never,
  ) => Result<TData, TError> | Promise<Result<TData, TError>>,
): { queryKey: QueryKey; queryFn: () => Promise<TData> } {
  return {
    queryKey: options.queryKey,
    queryFn: async () => unwrapResult(await queryFn(undefined as never)),
  }
}

export function invokeQuery<T>(options: {
  queryFn?: (context: never) => T | Promise<T>
}): T | Promise<T> {
  return options.queryFn!(undefined as never)
}

const NON_RETRYABLE_RPC_KINDS = new Set([
  'unauthorized',
  'forbidden',
  'unsupported',
  'unknown_method',
  'invalid_params',
])

export function isNonRetryableRpcFailure(error: unknown): boolean {
  return (
    error instanceof RpcFailure &&
    NON_RETRYABLE_RPC_KINDS.has(error.envelope.kind)
  )
}

export function shouldRetryRpcQuery(
  failureCount: number,
  error: unknown,
): boolean {
  if (isNonRetryableRpcFailure(error)) {
    return false
  }
  return failureCount < 3
}

/** The request lost its reply; this does not establish whether the source committed. */
export class MutationUnconfirmedError extends Error {
  constructor(cause: unknown) {
    super(
      'The operation result is unconfirmed. Inspect its current status before trying again.',
      { cause },
    )
    this.name = 'MutationUnconfirmedError'
  }
}

export async function invokeMutation<TInput, TOutput>(
  options: {
    mutationFn?: (input: TInput, context: never) => TOutput | Promise<TOutput>
  },
  input: TInput,
): Promise<TOutput> {
  try {
    return await options.mutationFn!(input, undefined as never)
  } catch (error) {
    // A reported RPC refusal is confirmed; only invoke-layer failures are
    // unconfirmed, while local programming errors propagate.
    if (
      error instanceof RpcFailure ||
      error instanceof TypeError ||
      error instanceof ReferenceError ||
      error instanceof SyntaxError ||
      error instanceof RangeError
    ) {
      throw error
    }
    throw new MutationUnconfirmedError(error)
  }
}
