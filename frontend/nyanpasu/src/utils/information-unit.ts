import { m } from '@/paraglide/messages'
import { getLocale } from '@/paraglide/runtime'

/** IEC byte units, from bytes to yobibytes. */
export const byteUnits = [
  'B',
  'KiB',
  'MiB',
  'GiB',
  'TiB',
  'PiB',
  'EiB',
  'ZiB',
  'YiB',
] as const

export type ByteUnit = (typeof byteUnits)[number]

const byteLabels = {
  B: m.unit_b,
  KiB: m.unit_kib,
  MiB: m.unit_mib,
  GiB: m.unit_gib,
  TiB: m.unit_tib,
  PiB: m.unit_pib,
  EiB: m.unit_eib,
  ZiB: m.unit_zib,
  YiB: m.unit_yib,
} satisfies Record<ByteUnit, () => string>

export const byteUnitLabel = (unit: ByteUnit) => byteLabels[unit]()

/**
 * Symbols for every unit `filesize` can display here. The SI bit units are
 * included because the traffic widgets can switch their rate display to bits.
 */
let cached: { locale: string; symbols: Record<string, string> } | undefined

export const informationUnitSymbols = (): Record<string, string> => {
  const locale = getLocale()

  if (cached?.locale !== locale) {
    cached = {
      locale,
      symbols: {
        B: byteUnitLabel('B'),
        KiB: byteUnitLabel('KiB'),
        MiB: byteUnitLabel('MiB'),
        GiB: byteUnitLabel('GiB'),
        TiB: byteUnitLabel('TiB'),
        PiB: byteUnitLabel('PiB'),
        EiB: byteUnitLabel('EiB'),
        ZiB: byteUnitLabel('ZiB'),
        YiB: byteUnitLabel('YiB'),
        bit: m.unit_bit(),
        kbit: m.unit_kbit(),
        Mbit: m.unit_mbit(),
        Gbit: m.unit_gbit(),
        Tbit: m.unit_tbit(),
        Pbit: m.unit_pbit(),
        Ebit: m.unit_ebit(),
        Zbit: m.unit_zbit(),
        Ybit: m.unit_ybit(),
      },
    }
  }

  return cached.symbols
}
