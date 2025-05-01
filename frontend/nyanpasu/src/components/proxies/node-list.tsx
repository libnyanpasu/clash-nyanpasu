import { AnimatePresence, motion } from 'framer-motion'
import { useAtomValue } from 'jotai'
import {
  forwardRef,
  RefObject,
  useCallback,
  useDeferredValue,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from 'react'
import { Virtualizer, VListHandle } from 'virtua'
import { proxyGroupAtom, proxyGroupSortAtom } from '@/store'
import { proxiesFilterAtom } from '@/store/proxies'
import {
  ClashProxiesQueryProxyItem,
  ProxyGroupItem,
  useClashProxies,
  useProxyMode,
  useSetting,
} from '@nyanpasu/interface'
import { cn, useBreakpointValue } from '@nyanpasu/ui'
import NodeCard from './node-card'
import { nodeSortingFn } from './utils'

type RenderClashProxy = ClashProxiesQueryProxyItem & { renderLayoutKey: string }

export interface NodeListRef {
  scrollToCurrent: () => void
}

export const NodeList = forwardRef(function NodeList(
  { scrollRef }: { scrollRef: RefObject<HTMLElement> },
  ref,
) {
  const { data } = useClashProxies()

  const { value: proxyMode } = useProxyMode()

  const proxyGroup = useAtomValue(proxyGroupAtom)
  const proxiesFilter = useAtomValue(proxiesFilterAtom)
  const deferredProxiesFilter = useDeferredValue(proxiesFilter)

  const proxyGroupSort = useAtomValue(proxyGroupSortAtom)

  const [group, setGroup] = useState<ProxyGroupItem>()

  const sortGroup = useCallback(() => {
    if (!proxyMode.global) {
      if (proxyGroup.selector !== null) {
        // eslint-disable-next-line @typescript-eslint/no-non-null-asserted-optional-chain
        const selectedGroup = data?.groups[proxyGroup.selector]!

        if (selectedGroup) {
          setGroup(nodeSortingFn(selectedGroup, proxyGroupSort))
        }
      }
    } else {
      if (data?.global) {
        setGroup(nodeSortingFn(data?.global, proxyGroupSort))
      } else {
        setGroup(data?.global)
      }
    }
  }, [
    proxyMode.global,
    proxyGroup.selector,
    data?.groups,
    data?.global,
    proxyGroupSort,
  ])

  useEffect(() => {
    sortGroup()
  }, [sortGroup])

  const column = useBreakpointValue({
    xs: 1,
    sm: 1,
    md: 2,
    lg: 3,
    xl: 4,
  })

  const [renderList, setRenderList] = useState<RenderClashProxy[][]>([])

  useEffect(() => {
    if (!group?.all) return

    const nodeNames: string[] = []

    let nodes = group?.all || []
    if (!!deferredProxiesFilter && deferredProxiesFilter !== group?.name) {
      nodes = nodes.filter((node) =>
        node.name.toLowerCase().includes(deferredProxiesFilter.toLowerCase()),
      )
    }

    const list = nodes.reduce<RenderClashProxy[][]>((result, value, index) => {
      const getKey = () => {
        const filter = nodeNames.filter((i) => i === value.name)

        if (filter.length === 0) {
          return value.name
        } else {
          return `${value.name}-${filter.length}`
        }
      }

      if (index % column === 0) {
        result.push([])
      }

      result[Math.floor(index / column)].push({
        ...(value as ClashProxiesQueryProxyItem),
        renderLayoutKey: getKey(),
      })

      nodeNames.push(value.name)

      return result
    }, [])

    setRenderList(list)
  }, [group?.all, group?.name, column, deferredProxiesFilter])

  const { value: disableMotion } = useSetting('lighten_animation_effects')

  const vListRef = useRef<VListHandle>(null)

  useImperativeHandle(ref, () => ({
    scrollToCurrent: () => {
      const index = renderList.findIndex((node) =>
        node.some((item) => item.name === group?.now),
      )

      vListRef.current?.scrollToIndex(index, {
        align: 'center',
        smooth: true,
      })
    },
  }))

  return (
    <AnimatePresence initial={false} mode="sync">
      <Virtualizer ref={vListRef} scrollRef={scrollRef}>
        {renderList?.map((node, index) => {
          return (
            <div
              key={index}
              className={cn('grid gap-2 px-2 pb-2', index === 0 && 'pt-14')}
              style={{ gridTemplateColumns: `repeat(${column} , 1fr)` }}
            >
              {node.map((render) => {
                const Card = () => (
                  <NodeCard
                    node={render}
                    now={group?.now}
                    disabled={group?.type !== 'Selector'}
                  />
                )

                return disableMotion ? (
                  <div className="relative overflow-hidden">
                    <Card />
                  </div>
                ) : (
                  <motion.div
                    key={render.name}
                    layoutId={`node-${render.renderLayoutKey}`}
                    className="relative overflow-hidden"
                    layout="position"
                    initial={false}
                  >
                    <Card />
                  </motion.div>
                )
              })}
            </div>
          )
        })}
      </Virtualizer>
    </AnimatePresence>
  )
})
