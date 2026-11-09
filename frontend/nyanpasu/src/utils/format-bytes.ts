import { filesize, type FilesizeOptions } from 'filesize'
import { informationUnitSymbols } from '@/utils/information-unit'

/**
 * `filesize` with IEC byte units by default. Number formatting and unit
 * selection remain in `filesize`; only the displayed symbols are localized.
 */
export const formatBytes = (
  value: number | string | bigint,
  options: FilesizeOptions = {},
) =>
  filesize(value, {
    standard: 'iec',
    ...options,
    symbols: informationUnitSymbols(),
  })
