/**
 * Retry a function with exponential backoff
 *
 * @param fn - The function to retry
 * @param options - Retry options
 * @returns The result of the function
 * @throws The last error encountered
 */
export async function retry<T>(
  fn: () => Promise<T>,
  options: {
    maxRetries?: number
    initialDelay?: number
    maxDelay?: number
    factor?: number
    retryCondition?: (error: Error) => boolean
  } = {},
): Promise<T> {
  const {
    maxRetries = 3,
    initialDelay = 200,
    maxDelay = 5000,
    factor = 2,
    retryCondition = () => true,
  } = options

  let lastError: Error = new Error('Unknown error occurred')

  let delay = initialDelay

  for (let attempt = 0; attempt <= maxRetries; attempt++) {
    try {
      return await fn()
    } catch (error) {
      if (attempt === maxRetries || !retryCondition(error as Error)) {
        throw error
      }

      lastError = error as Error

      // Wait for the specified delay
      await new Promise((resolve) => setTimeout(resolve, delay))

      // Increase the delay for the next attempt (exponential backoff)
      delay = Math.min(delay * factor, maxDelay)
    }
  }

  throw lastError
}
