import { useBreakpoint } from '@nyanpasu/ui'

export default function useIsMobile() {
  const breakpoint = useBreakpoint()

  const isMobile = breakpoint === 'sm' || breakpoint === 'xs'

  return isMobile
}

export function useIsMobileOrTablet() {
  const breakpoint = useBreakpoint()

  const isMobileOrTablet =
    breakpoint === 'sm' || breakpoint === 'xs' || breakpoint === 'md'

  return isMobileOrTablet
}
