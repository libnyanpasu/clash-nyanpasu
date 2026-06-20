import { useMemo } from 'react'
import { commands, unwrapResult, useProfile } from '@nyanpasu/interface'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  extractGroupNameFromMergeYaml,
  insertRuleIntoMergeYaml,
} from '../../proxies/group/_modules/group-builder'

export interface GuiGroup {
  uid: string
  groupName: string
}

/**
 * Discovers GUI-created proxy groups in the primary profile's chain and exposes
 * a mutation to append a rule into a group's merge file (then re-apply config).
 */
export const useGroupRules = () => {
  const queryClient = useQueryClient()
  const { query } = useProfile()

  // Merge profiles referenced by the primary (first) active profile's chain.
  const chainMergeUids = useMemo(() => {
    const primaryUid = query.data?.current?.[0]
    const primary = query.data?.items?.find((item) => item.uid === primaryUid)

    if (!primary || (primary.type !== 'local' && primary.type !== 'remote')) {
      return []
    }

    const chain = new Set(primary.chain ?? [])

    return (query.data?.items ?? [])
      .filter((item) => item.type === 'merge' && chain.has(item.uid))
      .map((item) => item.uid)
  }, [query.data])

  const groupsQuery = useQuery({
    queryKey: ['gui-groups', chainMergeUids],
    enabled: chainMergeUids.length > 0,
    queryFn: async () => {
      const groups: GuiGroup[] = []

      for (const uid of chainMergeUids) {
        const content = unwrapResult(await commands.readProfileFile(uid))
        const groupName = extractGroupNameFromMergeYaml(content ?? '')

        if (groupName) {
          groups.push({ uid, groupName })
        }
      }

      return groups
    },
  })

  const addRule = useMutation({
    mutationFn: async ({
      uid,
      ruleLine,
    }: {
      uid: string
      ruleLine: string
    }) => {
      const content = unwrapResult(await commands.readProfileFile(uid)) ?? ''
      const next = insertRuleIntoMergeYaml(content, ruleLine)

      unwrapResult(await commands.saveProfileFile(uid, next))
      unwrapResult(await commands.enhanceProfiles())
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['gui-groups'] })
    },
  })

  return { groupsQuery, addRule }
}
