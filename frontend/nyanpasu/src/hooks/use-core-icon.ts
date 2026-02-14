import ClashRs from '@/assets/image/core/clash-rs.png'
import ClashMeta from '@/assets/image/core/clash.meta.png'
import Clash from '@/assets/image/core/clash.png'
import { ClashCore } from '@nyanpasu/interface'

export default function useCoreIcon(core?: ClashCore | null) {
  switch (core) {
    case 'clash':
      return Clash
    case 'clash-rs':
    case 'clash-rs-alpha':
      return ClashRs
    case 'mihomo':
    case 'mihomo-alpha':
      return ClashMeta
    // sync from backend
    default:
      return ClashMeta
  }
}
