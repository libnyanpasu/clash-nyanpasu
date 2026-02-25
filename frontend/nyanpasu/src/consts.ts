/* eslint-disable */
// @ts-nocheck

import { getSystem } from '@nyanpasu/ui'

export const OS = getSystem()

export const isWindows = OS === 'windows'

export const isMacOS = OS === 'macos'

export const isLinux = OS === 'linux'

export const IS_NIGHTLY = window.__IS_NIGHTLY__ === true
