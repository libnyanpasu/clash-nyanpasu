import {
  createContext,
  PropsWithChildren,
  useCallback,
  useContext,
  useRef,
  useState,
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

interface BlockTaskContextType {
  tasks: Record<string, BlockTask>
  run: <T>(key: string, fn: () => Promise<T>) => Promise<T>
  getTask: (key: string) => BlockTask | undefined
  clearTask: (key: string) => void
}

const BlockContext = createContext<BlockTaskContextType | null>(null)

export const useBlockTaskContext = () => {
  const context = useContext(BlockContext)

  if (!context) {
    throw new Error('useBlockContext must be used within a BlockProvider')
  }

  return context
}

export const useBlockTask = <T,>(key: string, fn: () => Promise<T>) => {
  const { run, tasks } = useBlockTaskContext()

  const execute = useLockFn(async () => {
    return await run(key, fn)
  })

  return {
    execute,
    isPending: tasks[key]?.status === 'pending',
    isSuccess: tasks[key]?.status === 'success',
    isError: tasks[key]?.status === 'error',
    data: tasks[key]?.data,
    error: tasks[key]?.error,
  }
}

export const BlockTaskProvider = ({ children }: PropsWithChildren) => {
  const [tasks, setTasks] = useState<Record<string, BlockTask>>({})

  const tasksRef = useRef<Record<string, BlockTask>>({})

  const run = useCallback(
    async <T,>(key: string, fn: () => Promise<T>): Promise<T> => {
      const task: BlockTask<T> = {
        id: key,
        status: 'pending',
        startTime: Date.now(),
      }

      setTasks((prev) => ({ ...prev, [key]: task }))
      tasksRef.current[key] = task

      try {
        const data = await fn()

        const successTask: BlockTask<T> = {
          ...task,
          status: 'success',
          data,
          endTime: Date.now(),
        }

        setTasks((prev) => ({
          ...prev,
          [key]: successTask,
        }))

        tasksRef.current[key] = successTask

        return data
      } catch (error) {
        const errorTask: BlockTask = {
          ...task,
          status: 'error',
          error: error instanceof Error ? error : new Error(String(error)),
          endTime: Date.now(),
        }

        setTasks((prev) => ({
          ...prev,
          [key]: errorTask,
        }))

        tasksRef.current[key] = errorTask

        throw error
      }
    },
    [],
  )

  const getTask = useCallback((key: string) => tasks[key], [tasks])

  const clearTask = useCallback((key: string) => {
    setTasks((prev) => {
      const newTasks = { ...prev }
      delete newTasks[key]
      return newTasks
    })

    delete tasksRef.current[key]
  }, [])

  return (
    <BlockContext.Provider
      value={{
        tasks,
        run,
        getTask,
        clearTask,
      }}
    >
      {children}
    </BlockContext.Provider>
  )
}
