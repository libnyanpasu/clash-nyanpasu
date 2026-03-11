let listener: (() => void) | null = null
let done = false

/** Register a callback to run when the app signals splash is done. Returns unsubscribe fn. */
export const onSplashDone = (cb: () => void): (() => void) => {
  if (done) {
    // oxlint-disable-next-line promise/no-callback-in-promise
    Promise.resolve().then(cb)
    return () => {}
  }

  listener = cb

  return () => {
    if (listener === cb) {
      listener = null
    }
  }
}

/** Signal that the app has finished loading. Safe to call multiple times. */
export const signalSplashDone = () => {
  if (!done) {
    done = true
    listener?.()
    listener = null
  }
}
