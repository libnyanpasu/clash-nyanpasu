import { useMatches } from '@tanstack/react-router'

declare module '@tanstack/react-router' {
  interface StaticDataRouteOption {
    /**
     * `fill` opts a tool page out of the scrolling content surface of its
     * layout: the page fills the viewport and every region in it scrolls
     * internally. A route without it scrolls as a whole.
     */
    layout?: 'fill'
  }
}

/** The layout the leaf route of the current match chain asks for. */
export const useLeafRouteLayout = () =>
  useMatches({ select: (matches) => matches.at(-1)?.staticData.layout })
