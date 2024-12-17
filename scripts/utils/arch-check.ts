import { colorize, consola } from './logger'

export const archCheck = (platform: string, arch: string) => {
  consola.debug(colorize`platform {yellow ${platform}}`)

  consola.debug(colorize`arch {yellow ${arch}}`)
}
