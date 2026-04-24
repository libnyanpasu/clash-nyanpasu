type Platform =
  | 'aix'
  | 'android'
  | 'darwin'
  | 'freebsd'
  | 'haiku'
  | 'linux'
  | 'openbsd'
  | 'sunos'
  | 'win32'
  | 'cygwin'
  | 'netbsd'

declare const OS_PLATFORM: Platform | undefined

export function getSystem() {
  const userAgent =
    typeof window === 'undefined' ? '' : window.navigator?.userAgent
  const platform = typeof OS_PLATFORM !== 'undefined' ? OS_PLATFORM : 'unknown'

  if (userAgent.includes('Mac OS X') || platform === 'darwin') {
    return 'macos'
  }

  if (/win64|win32/i.test(userAgent) || platform === 'win32') {
    return 'windows'
  }

  if (/linux/i.test(userAgent) || platform === 'linux') {
    return 'linux'
  }

  return 'unknown'
}
