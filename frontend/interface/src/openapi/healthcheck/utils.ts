import { urlDelayTest } from '@/service'

export const timing = async (url: string, code: number) => {
  return (await urlDelayTest(url, code)) ?? 0
}

export const createTiming = (url: string, code: number = 204) => {
  return () => timing(url, code)
}
