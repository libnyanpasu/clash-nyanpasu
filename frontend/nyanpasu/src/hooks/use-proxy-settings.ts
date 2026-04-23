import { useBlockTask } from '@/components/providers/block-task-provider'
import { useSetting } from '@nyanpasu/interface'

export const useSystemProxy = () => {
  const systemProxy = useSetting('enable_system_proxy')

  const blockTask = useBlockTask('system-proxy', async () => {
    await systemProxy.upsert(!systemProxy.value)
  })

  return {
    ...blockTask,
    isActive: Boolean(systemProxy.value),
  }
}

export const useTunMode = () => {
  const tunMode = useSetting('enable_tun_mode')

  const blockTask = useBlockTask('tun-mode', async () => {
    await tunMode.upsert(!tunMode.value)
  })

  return {
    ...blockTask,
    isActive: Boolean(tunMode.value),
  }
}
