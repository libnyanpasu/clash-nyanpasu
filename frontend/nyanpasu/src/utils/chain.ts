export function chains<T>(
  ...handlers: Array<((event: T) => void) | undefined>
) {
  return (event: T) => {
    handlers.forEach((handler) => {
      if (handler) {
        handler(event)
      }
    })
  }
}
