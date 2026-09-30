import type { SVGProps } from 'react'

// Browser tests do not run unplugin-icons, so every `~icons/*` import renders
// nothing.
const IconStub: (props: SVGProps<SVGSVGElement>) => null = () => null

export default IconStub
