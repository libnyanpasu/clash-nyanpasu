import { useCallback } from 'react'

export function chains<T>(
  ...handlers: Array<((event: T) => void) | undefined>
) {
  const fn = useCallback(
    (event: T) => {
      handlers.forEach((handler) => {
        if (handler) {
          handler(event)
        }
      })
    },
    [handlers],
  )

  return fn
}
