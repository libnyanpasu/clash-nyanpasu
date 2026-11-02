import { setTimeout as delay } from 'node:timers/promises'

export class RetryError extends Error {
  constructor(cause: unknown, attempts: number) {
    super(`Retrying exceeded the maxAttempts (${attempts}).`, { cause })
    this.name = 'RetryError'
  }
}

export interface RetryOptions {
  maxAttempts?: number
  isRetriable?: (error: unknown) => boolean
}

const MIN_TIMEOUT_MS = 1000
const MAX_TIMEOUT_MS = 60000
const MULTIPLIER = 2

/**
 * Run `fn` until it succeeds, waiting with jittered exponential backoff
 * between attempts. A non-retriable error is thrown as is; running out of
 * attempts throws a RetryError that carries the last error as its cause.
 */
export async function retry<T>(
  fn: () => Promise<T> | T,
  { maxAttempts = 5, isRetriable = () => true }: RetryOptions = {},
): Promise<T> {
  for (let attempt = 0; ; attempt++) {
    try {
      return await fn()
    } catch (error) {
      if (!isRetriable(error)) throw error
      if (attempt + 1 >= maxAttempts) throw new RetryError(error, maxAttempts)

      const backoff = Math.min(
        MAX_TIMEOUT_MS,
        MIN_TIMEOUT_MS * MULTIPLIER ** attempt,
      )
      await delay((1 - Math.random()) * backoff)
    }
  }
}
