import { commands } from '@interface/ipc'
import { unwrapResult } from '@interface/utils'

export const timing = async (url: string, code: number) => {
  return (unwrapResult(await commands.urlDelayTest(url, code)) ?? 0) as number
}

export const createTiming = (url: string, code: number = 204) => {
  return () => timing(url, code)
}
