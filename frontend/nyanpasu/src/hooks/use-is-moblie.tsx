import { useBreakpoint } from '@nyanpasu/ui'

export default function useIsMobile() {
  const breakpoint = useBreakpoint()

  const isMobile = breakpoint === 'sm' || breakpoint === 'xs'

  return isMobile
}
