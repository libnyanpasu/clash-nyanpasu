/* eslint-disable @typescript-eslint/no-explicit-any */
import { useCallback, useRef } from 'react'

export type LockFn<P extends any[] = any[], T = any> = (
  ...args: P
) => Promise<T>

/**
 * Hook similar to ahooks useLockFn - prevents concurrent execution of async functions
 * When the function is executing, subsequent calls will be ignored until the function completes
 */
export function useLockFn<P extends any[] = any[], T = any>(
  fn: LockFn<P, T>,
): LockFn<P, T> {
  const lockRef = useRef(false)
  const fnRef = useRef(fn)

  // Update ref on each render to ensure we have the latest fn
  fnRef.current = fn

  return useCallback(async (...args: P): Promise<T> => {
    if (lockRef.current) {
      // return Promise.reject(new Error("Function is locked"));
      console.warn(`Function is locked: ${fnRef.current.name}`)
      return undefined as T
    }

    lockRef.current = true

    try {
      const result = await fnRef.current(...args)
      return result
    } finally {
      lockRef.current = false
    }
  }, [])
}
