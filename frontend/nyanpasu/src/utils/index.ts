import { includes, isArray, isObject, isString, some } from 'lodash-es'
import { EnvInfos } from '@nyanpasu/interface'

/**
 * classNames filter out falsy values and join the rest with a space
 * @param classes - array of classes
 * @returns string of classes
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function classNames(...classes: any[]) {
  return classes.filter(Boolean).join(' ')
}

export async function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

export const containsSearchTerm = (obj: any, term: string): boolean => {
  if (!obj || !term) return false

  if (isString(obj)) {
    return includes(obj.toLowerCase(), term.toLowerCase())
  }

  if (isObject(obj) || isArray(obj)) {
    return some(obj, (value: any) => containsSearchTerm(value, term))
  }

  return false
}

export function formatError(err: unknown): string {
  return `Error: ${err instanceof Error ? err.message : String(err)}`
}

export function formatEnvInfos(envs: EnvInfos) {
  let result = '----------- System -----------\n'
  result += `OS: ${envs.os}\n`
  result += `Arch: ${envs.arch}\n`
  result += `----------- Device -----------\n`
  for (const cpu of envs.device.cpu) {
    result += `CPU: ${cpu}\n`
  }
  result += `Memory: ${envs.device.memory}\n`
  result += `----------- Core -----------\n`
  for (const key in envs.core) {
    result += `${key}: \`${envs.core[key]}\`\n`
  }
  result += `----------- Build Info -----------\n`
  for (const k of Object.keys(envs.build_info) as string[]) {
    const key = k
      .split('_')
      .map((v) => v.charAt(0).toUpperCase() + v.slice(1))
      .join(' ')
    result += `${key}: ${envs.build_info[k]}\n`
  }
  return result
}
