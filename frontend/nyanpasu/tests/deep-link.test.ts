import { expect, test } from 'vitest'
import { receiveDeepLinks } from '../src/utils/deep-link.ts'

/** A backend queue whose listener registration the test completes by hand. */
const fakeQueue = () => {
  const log: string[] = []
  let queued: string[] = []
  let onPoke: (() => void) | undefined
  let register!: () => void
  const registered = new Promise<void>((resolve) => {
    register = resolve
  })
  let failNextTake = false
  let failListen = false
  return {
    log,
    push: (...links: string[]) => {
      queued.push(...links)
    },
    poke: () => onPoke?.(),
    register,
    failNextTake: () => {
      failNextTake = true
    },
    failListen: () => {
      failListen = true
    },
    queue: {
      listen: async (poke: () => void) => {
        log.push('listen')
        if (failListen) {
          throw new Error('listen failed')
        }
        onPoke = poke
        await registered
        log.push('listening')
        return () => {
          log.push('unlisten')
        }
      },
      take: async () => {
        log.push('take')
        if (failNextTake) {
          failNextTake = false
          throw new Error('take failed')
        }
        const links = queued
        queued = []
        return links
      },
    },
  }
}

/** A registered queue whose takes answer only when the test answers them. */
const heldQueue = () => {
  const takes: Array<(links: string[]) => void> = []
  let onPoke: (() => void) | undefined
  return {
    takes,
    poke: () => onPoke?.(),
    queue: {
      listen: async (poke: () => void) => {
        onPoke = poke
        return () => {}
      },
      take: () =>
        new Promise<string[]>((resolve) => {
          takes.push(resolve)
        }),
    },
  }
}

/** A handler whose calls finish only when the test finishes them. */
const heldHandler = () => {
  const started: string[] = []
  const settlers: Array<{
    resolve: () => void
    reject: (error: Error) => void
  }> = []
  return {
    started,
    finish: (index: number) => settlers[index].resolve(),
    fail: (index: number) => settlers[index].reject(new Error('import failed')),
    handle: (raw: string) =>
      new Promise<void>((resolve, reject) => {
        started.push(raw)
        settlers.push({ resolve, reject })
      }),
  }
}

const rethrow = (error: unknown) => {
  throw error
}

// Lets every pending promise callback run; no timer is waited on.
const settle = () => new Promise<void>((resolve) => setTimeout(resolve))

test('links queued before the listener registers are drained right after it', async () => {
  const backend = fakeQueue()
  const handled: string[] = []
  backend.push('a', 'b')
  receiveDeepLinks(
    backend.queue,
    async (raw) => {
      handled.push(raw)
    },
    (error) => {
      throw error
    },
  )

  await settle()
  expect(backend.log).toEqual(['listen'])
  backend.push('c')

  backend.register()
  await settle()
  expect(backend.log).toEqual(['listen', 'listening', 'take'])
  expect(handled).toEqual(['a', 'b', 'c'])
})

test('every poke drains the queue again, oldest first', async () => {
  const backend = fakeQueue()
  const handled: string[] = []
  const errors: unknown[] = []
  receiveDeepLinks(
    backend.queue,
    async (raw) => {
      if (raw === 'bad') {
        throw new Error('import failed')
      }
      handled.push(raw)
    },
    (error) => errors.push(error),
  )
  backend.register()
  await settle()

  backend.push('d')
  backend.poke()
  await settle()
  expect(handled).toEqual(['d'])

  backend.poke()
  await settle()
  expect(handled).toEqual(['d'])

  backend.failNextTake()
  backend.push('e')
  backend.poke()
  await settle()
  expect(handled).toEqual(['d'])
  expect(errors).toHaveLength(1)

  // A failed take leaves `e` queued; a failed import does not stop `f`.
  backend.push('bad', 'f')
  backend.poke()
  await settle()
  expect(handled).toEqual(['d', 'e', 'f'])
  expect(errors).toHaveLength(2)
  expect(backend.log.filter((entry) => entry === 'take')).toHaveLength(5)
})

test('a caller gone before the listener registers leaves the queue alone', async () => {
  const backend = fakeQueue()
  backend.push('a')
  const stop = receiveDeepLinks(
    backend.queue,
    async () => {
      throw new Error('nothing may be handled')
    },
    (error) => {
      throw error
    },
  )
  stop()

  backend.register()
  await settle()
  backend.poke()
  await settle()
  expect(backend.log).toEqual(['listen', 'listening', 'unlisten'])
})

test('stopping unregisters the listener and ignores later pokes', async () => {
  const backend = fakeQueue()
  const stop = receiveDeepLinks(
    backend.queue,
    async () => {},
    (error) => {
      throw error
    },
  )
  backend.register()
  await settle()

  stop()
  backend.push('a')
  backend.poke()
  await settle()
  expect(backend.log).toEqual(['listen', 'listening', 'take', 'unlisten'])
})

test('a take is never sent while another is unanswered, so answers cannot arrive out of order', async () => {
  const backend = heldQueue()
  const handled: string[] = []
  receiveDeepLinks(
    backend.queue,
    async (raw) => {
      handled.push(raw)
    },
    rethrow,
  )
  await settle()
  expect(backend.takes).toHaveLength(1)

  // Pokes during the take only ask for one more once it is answered.
  backend.poke()
  backend.poke()
  backend.poke()
  await settle()
  expect(backend.takes).toHaveLength(1)

  backend.takes[0](['a'])
  await settle()
  expect(handled).toEqual(['a'])
  expect(backend.takes).toHaveLength(2)

  backend.takes[1](['b'])
  await settle()
  expect(handled).toEqual(['a', 'b'])
  expect(backend.takes).toHaveLength(2)
})

test('a batch is handled one link at a time, each link once, before the next take', async () => {
  const backend = fakeQueue()
  const handler = heldHandler()
  backend.push('a', 'b', 'a')
  receiveDeepLinks(backend.queue, handler.handle, rethrow)
  backend.register()
  await settle()
  expect(handler.started).toEqual(['a'])

  backend.push('c')
  backend.poke()
  await settle()
  expect(handler.started).toEqual(['a'])

  handler.finish(0)
  await settle()
  expect(handler.started).toEqual(['a', 'b'])

  handler.finish(1)
  await settle()
  expect(handler.started).toEqual(['a', 'b', 'c'])
  expect(backend.log.filter((entry) => entry === 'take')).toHaveLength(2)
})

test('a poke before the listener resolves is drained first, then the initial drain', async () => {
  const backend = fakeQueue()
  const handler = heldHandler()
  receiveDeepLinks(backend.queue, handler.handle, rethrow)
  await settle()

  // Tauri calls the listener before `listen` resolves.
  backend.push('a')
  backend.poke()
  await settle()
  expect(handler.started).toEqual(['a'])

  backend.push('b')
  backend.register()
  await settle()
  expect(handler.started).toEqual(['a'])

  handler.finish(0)
  await settle()
  expect(handler.started).toEqual(['a', 'b'])
  expect(backend.log).toEqual(['listen', 'take', 'listening', 'take'])
})

test('a listener that fails to register still drains the queue once', async () => {
  const backend = fakeQueue()
  const handled: string[] = []
  const errors: unknown[] = []
  backend.failListen()
  backend.push('a')
  receiveDeepLinks(
    backend.queue,
    async (raw) => {
      handled.push(raw)
    },
    (error) => errors.push(error),
  )
  await settle()

  expect(handled).toEqual(['a'])
  expect(errors).toHaveLength(1)
  expect(backend.log).toEqual(['listen', 'take'])
})

test('a copy queued while its link is being handled is not handled again', async () => {
  const backend = fakeQueue()
  const handler = heldHandler()
  backend.push('a')
  receiveDeepLinks(backend.queue, handler.handle, rethrow)
  backend.register()
  await settle()
  expect(handler.started).toEqual(['a'])

  // The user opens the same link again while its import is still running.
  backend.push('a')
  backend.poke()
  await settle()
  handler.finish(0)
  await settle()
  expect(handler.started).toEqual(['a'])
  expect(backend.log.filter((entry) => entry === 'take')).toHaveLength(2)

  // Opened again once that import finished, it is a new request.
  backend.push('a')
  backend.poke()
  await settle()
  expect(handler.started).toEqual(['a', 'a'])
})

test('a link that failed is retried when reopened while a later link of its batch runs', async () => {
  const backend = fakeQueue()
  const handler = heldHandler()
  const errors: unknown[] = []
  backend.push('a', 'b')
  receiveDeepLinks(backend.queue, handler.handle, (error) => errors.push(error))
  backend.register()
  await settle()
  handler.fail(0)
  await settle()
  expect(handler.started).toEqual(['a', 'b'])

  // `a` is done, so reopening it while `b` imports is a new request.
  backend.push('a')
  backend.poke()
  await settle()
  handler.finish(1)
  await settle()
  expect(handler.started).toEqual(['a', 'b', 'a'])
  expect(errors).toHaveLength(1)
})

// Known behavior, pinned so that changing the rule is a decision: a poke
// carries no URL, so the poke for `b` also marks `a`, which was running then.
test('known: a link reopened while a later link of its batch runs is dropped after a poke for another link', async () => {
  const backend = fakeQueue()
  const handler = heldHandler()
  const errors: unknown[] = []
  backend.push('a', 'c')
  receiveDeepLinks(backend.queue, handler.handle, (error) => errors.push(error))
  backend.register()
  await settle()
  expect(handler.started).toEqual(['a'])

  // A different link arrives while `a` imports.
  backend.push('b')
  backend.poke()
  await settle()
  handler.fail(0)
  await settle()
  expect(handler.started).toEqual(['a', 'c'])

  // `a` is reopened while `c` imports, so it shares the next take with `b`.
  backend.push('a')
  backend.poke()
  await settle()
  handler.finish(1)
  await settle()
  handler.finish(2)
  await settle()
  expect(handler.started).toEqual(['a', 'c', 'b'])
  expect(errors).toHaveLength(1)
})

test('a handler that throws at once still lets the batch and a pending poke through', async () => {
  const backend = fakeQueue()
  const handled: string[] = []
  const errors: unknown[] = []
  backend.push('x', 'b')
  receiveDeepLinks(
    backend.queue,
    (raw) => {
      if (raw === 'x') {
        backend.push('c')
        backend.poke()
        throw new Error('thrown before any promise')
      }
      handled.push(raw)
      return Promise.resolve()
    },
    (error) => errors.push(error),
  )
  backend.register()
  await settle()

  expect(handled).toEqual(['b', 'c'])
  expect(errors).toHaveLength(1)
})

test('a caller gone during an unanswered take still handles that batch and takes no more', async () => {
  const backend = heldQueue()
  const handled: string[] = []
  const stop = receiveDeepLinks(
    backend.queue,
    async (raw) => {
      handled.push(raw)
    },
    rethrow,
  )
  await settle()
  expect(backend.takes).toHaveLength(1)

  backend.poke()
  stop()
  backend.poke()
  backend.takes[0](['a', 'b'])
  await settle()

  expect(handled).toEqual(['a', 'b'])
  expect(backend.takes).toHaveLength(1)
})
