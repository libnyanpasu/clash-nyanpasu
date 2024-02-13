import { useEffect, useMemo } from "react";
import useSWR from "swr";
// import { getProxies } from "@/services/api";
import { useVerge } from "@/hooks/use-verge";
import { getProxies } from "@/services/cmds";
import { filterSort } from "./use-filter-sort";
import {
  DEFAULT_STATE,
  useHeadStateNew,
  type HeadState,
} from "./use-head-state";
import { useWindowWidth } from "./use-window-width";

export interface IRenderItem {
  // 组 ｜ head ｜ item ｜ empty | item col | item bottom | empty-padding
  type: 0 | 1 | 2 | 3 | 4 | 5 | 6;
  key: string;
  group?: IProxyGroupItem;
  proxy?: IProxyItem;
  col?: number;
  proxyCol?: IProxyItem[];
  headState?: HeadState;
}

export const useRenderList = (mode: string) => {
  const { data: proxiesData, mutate: mutateProxies } = useSWR(
    "getProxies",
    async () => {
      const res = await getProxies();
      console.log(res);
      return res;
    },
    { refreshInterval: 45000 },
  );

  const { verge } = useVerge();
  const { width } = useWindowWidth();

  let col = Math.floor(verge?.proxy_layout_column || 6);

  // 自适应
  if (col >= 6 || col <= 0) {
    if (width > 1450) col = 4;
    else if (width > 1024) col = 3;
    else if (width > 900) col = 2;
    else if (width >= 600) col = 1;
    else col = 1;
  }

  const [headStates, setHeadState] = useHeadStateNew();

  // make sure that fetch the proxies successfully
  useEffect(() => {
    if (!proxiesData) return;
    const { groups, proxies } = proxiesData;

    if (
      (mode === "rule" && !groups.length) ||
      (mode === "global" && proxies.length < 2)
    ) {
      setTimeout(() => mutateProxies(), 500);
    }
  }, [proxiesData, mode]);

  const renderList: IRenderItem[] = useMemo(() => {
    if (!proxiesData) return [];

    // global 和 direct 使用展开的样式
    const useRule = mode === "rule" || mode === "script";
    const renderGroups =
      (useRule && proxiesData.groups.length
        ? proxiesData.groups
        : [proxiesData.global!]) || [];

    const retList = renderGroups.flatMap((group) => {
      const headState = headStates[group.name] || DEFAULT_STATE;
      const ret: IRenderItem[] = [
        { type: 0, key: group.name, group, headState },
      ];

      if (headState?.open || !useRule) {
        const proxies = filterSort(
          group.all,
          group.name,
          headState.filterText,
          headState.sortType,
        );

        ret.push({ type: 1, key: `head-${group.name}`, group, headState });

        if (!proxies.length) {
          ret.push({ type: 3, key: `empty-${group.name}`, group, headState });
        }

        // 支持多列布局
        if (col > 1) {
          const lists = ret.concat(
            groupList(proxies, col).map((proxyCol) => ({
              type: 4,
              key: `col-${group.name}-${proxyCol[0].name}`,
              group,
              headState,
              col,
              proxyCol,
            })),
          );

          lists.push({
            type: 5,
            key: `footer-${group.name}`,
            group,
            headState,
          });

          return lists;
        }

        const lists = ret.concat(
          proxies.map((proxy) => ({
            type: 2,
            key: `${group.name}-${proxy!.name}`,
            group,
            proxy,
            headState,
          })),
        );

        lists.push({
          type: 5,
          key: `footer-${group.name}`,
        });

        return lists;
      }

      return ret;
    });

    if (!useRule) return retList.slice(1);

    console.log(retList);

    retList.push({
      type: 6,
      key: `empty-padding`,
    });

    return retList;
  }, [headStates, proxiesData, mode, col]);

  return {
    renderList,
    onProxies: mutateProxies,
    onHeadState: setHeadState,
  };
};

function groupList<T = any>(list: T[], size: number): T[][] {
  return list.reduce((p, n) => {
    if (!p.length) return [[n]];

    const i = p.length - 1;
    if (p[i].length < size) {
      p[i].push(n);
      return p;
    }

    p.push([n]);
    return p;
  }, [] as T[][]);
}
