import { useAtom, useAtomValue } from 'jotai'
import { memo, RefObject, useDeferredValue, useMemo } from 'react'
import useSWR from 'swr'
import { Virtualizer } from 'virtua'
import { proxyGroupAtom } from '@/store'
import { proxiesFilterAtom } from '@/store/proxies'
import {
  alpha,
  ListItem,
  ListItemButton,
  ListItemButtonProps,
  ListItemIcon,
  ListItemText,
  useTheme,
} from '@mui/material'
import { getServerPort, useClashCore } from '@nyanpasu/interface'
import { LazyImage } from '@nyanpasu/ui'

const IconRender = memo(function IconRender({ icon }: { icon: string }) {
  const {
    data: serverPort,
    isLoading,
    error,
  } = useSWR('/getServerPort', getServerPort)
  const src = icon.trim().startsWith('<svg')
    ? `data:image/svg+xml;base64,${btoa(icon)}`
    : icon
  const cachedUrl = useMemo(() => {
    if (!src.startsWith('http')) {
      return src
    }
    return `http://localhost:${serverPort}/cache/icon?url=${btoa(src)}`
  }, [src, serverPort])
  if (isLoading || error) {
    return null
  }
  return (
    <ListItemIcon>
      <LazyImage
        className="h-11 w-11"
        loadingClassName="rounded-full"
        src={cachedUrl}
      />
    </ListItemIcon>
  )
})

export interface GroupListProps extends ListItemButtonProps {
  scrollRef: RefObject<HTMLElement>
}

export const GroupList = ({
  scrollRef,
  ...listItemButtonProps
}: GroupListProps) => {
  const { data } = useClashCore()

  const { palette } = useTheme()

  const [proxyGroup, setProxyGroup] = useAtom(proxyGroupAtom)
  const proxiesFilter = useAtomValue(proxiesFilterAtom)
  const deferredProxiesFilter = useDeferredValue(proxiesFilter)

  const handleSelect = (index: number) => {
    setProxyGroup({ selector: index })
  }

  const groups = useMemo(() => {
    if (!data?.groups) {
      return []
    }

    return data.groups.filter((group) => {
      const filterMatches =
        !deferredProxiesFilter ||
        group.name
          .toLowerCase()
          .includes(deferredProxiesFilter.toLowerCase()) ||
        group.all?.some((proxy) => {
          return proxy.name
            .toLowerCase()
            .includes(deferredProxiesFilter.toLowerCase())
        }) ||
        false
      return !(group.hidden ?? false) && filterMatches
    })
  }, [data?.groups, deferredProxiesFilter])

  return (
    <Virtualizer scrollRef={scrollRef}>
      {groups.map((group, index) => {
        const selected = index === proxyGroup.selector

        return (
          <ListItem key={index} disablePadding>
            <ListItemButton
              selected={selected}
              onClick={() => handleSelect(index)}
              sx={[
                selected
                  ? {
                      backgroundColor: `${alpha(palette.primary.main, 0.3)} !important`,
                    }
                  : {
                      backgroundColor: null,
                    },
              ]}
              {...listItemButtonProps}
            >
              {group.icon && <IconRender icon={group.icon} />}

              <ListItemText
                className="!truncate"
                primary={group.name}
                secondary={group.now}
              />
            </ListItemButton>
          </ListItem>
        )
      })}
    </Virtualizer>
  )
}
