import { useLockFn } from 'ahooks'
import { useAtom } from 'jotai'
import { RefObject, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import ContentDisplay from '@/components/base/content-display'
import {
  DelayButton,
  GroupList,
  NodeList,
  NodeListRef,
} from '@/components/proxies'
import ProxyGroupName from '@/components/proxies/proxy-group-name'
import ScrollCurrentNode from '@/components/proxies/scroll-current-node'
import SortSelector from '@/components/proxies/sort-selector'
import { proxyGroupAtom } from '@/store'
import { proxiesFilterAtom } from '@/store/proxies'
import { Check } from '@mui/icons-material'
import {
  alpha,
  Box,
  Button,
  ButtonGroup,
  TextField,
  useTheme,
} from '@mui/material'
import {
  ProxyGroupItem,
  useClashProxies,
  useProxyMode,
} from '@nyanpasu/interface'
import { cn, SidePage } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/proxies')({
  component: ProxyPage,
})

function SideBar() {
  const { palette } = useTheme()
  const [proxiesFilter, setProxiesFilter] = useAtom(proxiesFilterAtom)
  const { t } = useTranslation()

  return (
    <TextField
      hiddenLabel
      fullWidth
      autoComplete="off"
      spellCheck="false"
      placeholder={t('Filter conditions')}
      className="!pb-0"
      sx={{ input: { py: 1.2, fontSize: 14 } }}
      value={proxiesFilter || ''}
      onChange={(e) =>
        setProxiesFilter(!e.target.value.trim().length ? null : e.target.value)
      }
      InputProps={{
        sx: {
          borderRadius: 7,
          backgroundColor: alpha(palette.primary.main, 0.1),

          fieldset: {
            border: 'none',
          },
        },
      }}
    />
  )
}

function ProxyPage() {
  const { t } = useTranslation()

  const { value: proxyMode, upsert } = useProxyMode()

  const { data } = useClashProxies()

  const [proxyGroup] = useAtom(proxyGroupAtom)

  const [group, setGroup] = useState<ProxyGroupItem>()

  useEffect(() => {
    if (proxyMode.global) {
      setGroup(data?.global)
    } else if (proxyMode.direct) {
      setGroup(data?.direct ? { ...data.direct, all: [] } : undefined)
    } else {
      if (proxyGroup.selector !== null) {
        setGroup(data?.groups[proxyGroup.selector])
      }
    }
  }, [
    proxyGroup.selector,
    data?.groups,
    data?.global,
    proxyMode.global,
    proxyMode.direct,
    data?.direct,
  ])

  const handleDelayClick = async () => {
    if (proxyMode.global) {
      await data?.global.mutateDelay()
    } else {
      if (proxyGroup.selector !== null) {
        await data?.groups[proxyGroup.selector].mutateDelay()
      }
    }
  }

  const hasProxies = Boolean(data?.groups.length)

  const nodeListRef = useRef<NodeListRef>(null)

  const handleSwitch = useLockFn(async (key: string) => {
    await upsert(key)
  })

  const Header = useMemo(() => {
    return (
      <Box display="flex" alignItems="center" gap={1}>
        <ButtonGroup size="small">
          {Object.entries(proxyMode).map(([key, enabled]) => (
            <Button
              key={key}
              variant={enabled ? 'contained' : 'outlined'}
              onClick={() => handleSwitch(key)}
              sx={{ textTransform: 'capitalize' }}
            >
              {enabled && <Check className="mr-[0.1rem] -ml-2 scale-75" />}
              {t(key)}
            </Button>
          ))}
        </ButtonGroup>
      </Box>
    )
  }, [handleSwitch, proxyMode, t])

  const leftViewportRef = useRef<HTMLDivElement>(null)

  const rightViewportRef = useRef<HTMLDivElement>(null)

  return (
    <SidePage
      title={t('Proxy Groups')}
      header={Header}
      sideBar={<SideBar />}
      leftViewportRef={leftViewportRef}
      rightViewportRef={rightViewportRef}
      side={
        hasProxies &&
        proxyMode.rule && (
          <GroupList scrollRef={leftViewportRef as RefObject<HTMLElement>} />
        )
      }
      portalRightRoot={
        hasProxies &&
        !proxyMode.direct && (
          <div
            className={cn(
              'absolute z-10 flex w-full items-center justify-between px-4 py-2 backdrop-blur',
              'bg-gray-200/30 dark:bg-gray-900/30',
              '!rounded-t-2xl',
            )}
          >
            <div className="flex items-center gap-4">
              {group?.name && <ProxyGroupName name={group?.name} />}
            </div>

            <div className="flex gap-2">
              <ScrollCurrentNode
                onClick={() => {
                  nodeListRef.current?.scrollToCurrent()
                }}
              />

              <SortSelector />
            </div>
          </div>
        )
      }
    >
      {!proxyMode.direct ? (
        hasProxies ? (
          <>
            <NodeList
              ref={nodeListRef}
              scrollRef={rightViewportRef as RefObject<HTMLElement>}
            />

            <DelayButton onClick={handleDelayClick} />
          </>
        ) : (
          <ContentDisplay className="absolute" message={t('No Proxies')} />
        )
      ) : (
        <ContentDisplay className="absolute" message={t('Direct Mode')} />
      )}
    </SidePage>
  )
}
