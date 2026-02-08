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

// get the system os
// according to UA
export function getSystem() {
  const ua = typeof window === 'undefined' ? '' : window.navigator?.userAgent
  const platform = typeof OS_PLATFORM !== 'undefined' ? OS_PLATFORM : 'unknown'

  if (ua.includes('Mac OS X') || platform === 'darwin') return 'macos'

  if (/win64|win32/i.test(ua) || platform === 'win32') return 'windows'

  if (/linux/i.test(ua)) return 'linux'

  return 'unknown'
}
