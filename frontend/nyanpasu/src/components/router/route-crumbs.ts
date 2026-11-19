import { useMatches } from '@tanstack/react-router'

export type CrumbMatch = { params: Record<string, string> }

/**
 * A route the crumb chain navigates to. A string keeps this type independent
 * of the route tree, which the crumb augmentation is part of; link call sites
 * check the static targets.
 */
export type CrumbTarget = {
  to: string
  params?: Record<string, string>
}

export type RouteCrumb = {
  /** Lazy and localized, so a locale change does not recreate the route. */
  title: (match: CrumbMatch) => string
  /** The route is a navigation hub whose sidebar stays visible on desktop. */
  hub?: true
  /** The semantic parent when the nearest crumb is not the navigation parent. */
  parent?: (match: CrumbMatch) => CrumbTarget
}

declare module '@tanstack/react-router' {
  interface StaticDataRouteOption {
    crumb?: RouteCrumb
  }
}

export type ResolvedCrumb = {
  crumb: RouteCrumb
  params: Record<string, string>
  fullPath: string
}

export type ResolvedCrumbTarget = CrumbTarget & {
  hub: boolean
  title: string
}

/** The matches that declare a crumb, outermost first. */
export const useRouteCrumbs = () =>
  useMatches({
    select: (matches): ResolvedCrumb[] =>
      matches.flatMap((match) => {
        const crumb = match.staticData.crumb

        return crumb
          ? [{ crumb, params: match.params, fullPath: match.fullPath }]
          : []
      }),
  })

const normalizePath = (path: string) =>
  path.length > 1 ? path.replace(/\/$/, '') : path

/**
 * The semantic parent of the leaf crumb: its explicit `parent`, or else the
 * nearest earlier crumb. The target is the match's own URL, so it includes
 * the dynamic parameters.
 */
export function resolveCrumbParent(
  crumbs: ResolvedCrumb[],
): ResolvedCrumbTarget | undefined {
  const leaf = crumbs.at(-1)

  if (!leaf) {
    return undefined
  }

  const override = leaf.crumb.parent?.({ params: leaf.params })

  if (override) {
    const target = crumbs.find(
      (crumb) => normalizePath(crumb.fullPath) === normalizePath(override.to),
    )
    const params = override.params ?? target?.params ?? {}

    return {
      ...override,
      params,
      hub: Boolean(target?.crumb.hub),
      title: target?.crumb.title({ params }) ?? '',
    }
  }

  const parent = crumbs.at(-2)

  if (!parent) {
    return undefined
  }

  return {
    to: parent.fullPath,
    params: parent.params,
    hub: Boolean(parent.crumb.hub),
    title: parent.crumb.title({ params: parent.params }),
  }
}

export const useCrumbParent = () => {
  const crumbs = useRouteCrumbs()

  return resolveCrumbParent(crumbs)
}

export const useLeafCrumb = () => useRouteCrumbs().at(-1)
