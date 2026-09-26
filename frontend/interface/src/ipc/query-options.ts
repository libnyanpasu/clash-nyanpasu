import type { QueryKey } from '@tanstack/react-query'
import { unwrapResult, type Result } from '../utils'

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
    // Domain refusals resolve as `{ status: 'error' }`; only invoke-layer
    // failures are unconfirmed, while local programming errors propagate.
    if (
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
