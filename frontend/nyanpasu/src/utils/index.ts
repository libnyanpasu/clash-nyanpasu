import { isIpcError } from '@nyanpasu/rpc'
import { EnvInfo } from '@nyanpasu/rpc/types'
import { ipcErrorMessage } from './ipc-error'
import type { ProfileLabel } from './profile-label'

export async function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms))
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
