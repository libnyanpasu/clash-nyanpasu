import {
  createContext,
  PropsWithChildren,
  useContext,
  useState,
  useSyncExternalStore,
} from 'react'
import { useLockFn } from '@/hooks/use-lock-fn'

type BlockTaskStatus = 'idle' | 'pending' | 'success' | 'error'

// eslint-disable-next-line @typescript-eslint/no-explicit-any
interface BlockTask<T = any> {
  id: string
  status: BlockTaskStatus
  data?: T
  error?: Error
  startTime: number
  endTime?: number
}

interface BlockTaskStore {
  getTasks: () => Record<string, BlockTask>
  subscribe: (listener: () => void) => () => void
  run: <T>(key: string, fn: () => Promise<T>) => Promise<T>
  clearTask: (key: string) => void
}

// Hooks subscribe through useSyncExternalStore with a per-key snapshot, so a
// task update re-renders only the hooks reading that task instead of every
// consumer of the provider (e.g. every proxy node during a delay test).
const createBlockTaskStore = (): BlockTaskStore => {
  let tasks: Record<string, BlockTask> = {}
  const listeners = new Set<() => void>()

  const setTask = (key: string, task: BlockTask | undefined) => {
    const next = { ...tasks }

    if (task) {
      next[key] = task
    } else {
      delete next[key]
    }

    tasks = next
    listeners.forEach((listener) => listener())
  }

  return {
    getTasks: () => tasks,
    subscribe: (listener) => {
      listeners.add(listener)

      return () => {
        listeners.delete(listener)
      }
    },
    run: async <T,>(key: string, fn: () => Promise<T>): Promise<T> => {
      const task: BlockTask<T> = {
        id: key,
        status: 'pending',
        startTime: Date.now(),
      }

      setTask(key, task)

      try {
        const data = await fn()

        setTask(key, {
          ...task,
          status: 'success',
          data,
          endTime: Date.now(),
        })

        return data
      } catch (error) {
        setTask(key, {
          ...task,
          status: 'error',
          error: error instanceof Error ? error : new Error(String(error)),
          endTime: Date.now(),
        })

        throw error
      }
    },
    clearTask: (key) => setTask(key, undefined),
  }
}

const BlockContext = createContext<BlockTaskStore | null>(null)

const useBlockTaskStore = () => {
  const store = useContext(BlockContext)

  if (!store) {
    throw new Error('useBlockContext must be used within a BlockProvider')
  }

  return store
}

export const useBlockTaskContext = () => {
  const store = useBlockTaskStore()

  const tasks = useSyncExternalStore(store.subscribe, store.getTasks)

  return {
    tasks,
    run: store.run,
    getTask: (key: string): BlockTask | undefined => tasks[key],
    clearTask: store.clearTask,
  }
}

export const useBlockTask = <T, Args extends unknown[] = []>(
  key: string,
  fn: (...args: Args) => Promise<T>,
) => {
  const store = useBlockTaskStore()

  const task = useSyncExternalStore(
    store.subscribe,
    () => store.getTasks()[key],
  )

  const execute = useLockFn(async (...args: Args) => {
    return await store.run(key, () => fn(...args))
  })

  return {
    execute,
    isPending: task?.status === 'pending',
    isSuccess: task?.status === 'success',
    isError: task?.status === 'error',
    data: task?.data,
    error: task?.error,
  }
}

export const BlockTaskProvider = ({ children }: PropsWithChildren) => {
  const [store] = useState(createBlockTaskStore)

  return <BlockContext.Provider value={store}>{children}</BlockContext.Provider>
}
