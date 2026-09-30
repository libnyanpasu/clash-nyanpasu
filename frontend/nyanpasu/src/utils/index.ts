// oxlint-disable typescript/no-explicit-any
import { EnvInfo, isIpcError } from '@nyanpasu/interface'
import { ipcErrorMessage } from './ipc-error'
import type { ProfileLabel } from './profile-label'

/**
 * classNames filter out falsy values and join the rest with a space
 * @param classes - array of classes
 * @returns string of classes
 */
export function classNames(...classes: any[]) {
  return classes.filter(Boolean).join(' ')
}

export async function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

export const containsSearchTerm = (obj: any, term: string): boolean => {
  if (!obj || !term) return false

  if (typeof obj === 'string') {
    return obj.toLowerCase().includes(term.toLowerCase())
  }

  if (typeof obj === 'object') {
    return Object.values(obj).some((value: any) =>
      containsSearchTerm(value, term),
    )
  }

  return false
}

/** The simplest message for a caught error; the original goes to "copy error details". */
export function formatError(err: unknown, profileLabel?: ProfileLabel): string {
  if (isIpcError(err)) {
    return ipcErrorMessage(err, profileLabel)
  }
  return err instanceof Error ? err.message : String(err)
}

export function formatEnvInfos(envs: EnvInfo) {
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
      .map((v: string) => v.charAt(0).toUpperCase() + v.slice(1))
      .join(' ')
    // Fix linter error: explicitly type k as keyof typeof envs.build_info
    result += `${key}: ${envs.build_info[k as keyof typeof envs.build_info]}\n`
  }

  return result
}
