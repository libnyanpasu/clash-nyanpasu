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

export function invokeMutation<TInput, TOutput>(
  options: {
    mutationFn?: (input: TInput, context: never) => TOutput | Promise<TOutput>
  },
  input: TInput,
): TOutput | Promise<TOutput> {
  return options.mutationFn!(input, undefined as never)
}
