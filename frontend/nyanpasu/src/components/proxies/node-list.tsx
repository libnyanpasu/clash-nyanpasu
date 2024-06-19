import { Clash, useClashCore, useNyanpasu } from "@nyanpasu/interface";
import { useBreakpoint } from "@nyanpasu/ui";
import { useAtom, useAtomValue } from "jotai";
import { proxyGroupAtom, proxyGroupSortAtom } from "@/store";
import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
  useTransition,
} from "react";
import { classNames } from "@/utils";
import { VList, VListHandle } from "virtua";
import { AnimatePresence, motion } from "framer-motion";
import { filterDelay } from "./utils";
import NodeCard from "./node-card";

type RenderClashProxy = Clash.Proxy<string> & { renderLayoutKey: string };

export interface NodeListRef {
  scrollToCurrent: () => void;
}

export const NodeList = forwardRef(function NodeList({}, ref) {
  const { data, setGroupProxy, setGlobalProxy, updateProxiesDelay } =
    useClashCore();

  const [isPending, startTransition] = useTransition();

  const { getCurrentMode } = useNyanpasu();

  const [proxyGroup] = useAtom(proxyGroupAtom);

  const proxyGroupSort = useAtomValue(proxyGroupSortAtom);

  const [group, setGroup] = useState<Clash.Proxy<Clash.Proxy<string>>>();

  const sortGroup = useCallback(() => {
    if (!getCurrentMode.global) {
      if (proxyGroup.selector !== null) {
        const selectedGroup = data?.groups[proxyGroup.selector];

        if (selectedGroup) {
          let sortedList = selectedGroup.all?.slice();

          if (proxyGroupSort === "delay") {
            sortedList = sortedList?.sort((a, b) => {
              const delayA = filterDelay(a.history);
              const delayB = filterDelay(b.history);

              if (delayA === -1 || delayA === -2) return 1;
              if (delayB === -1 || delayB === -2) return -1;

              if (delayA === 0) return 1;
              if (delayB === 0) return -1;

              return delayA - delayB;
            });
          } else if (proxyGroupSort === "name") {
            sortedList = sortedList?.sort((a, b) =>
              a.name.localeCompare(b.name),
            );
          }

          setGroup({
            ...selectedGroup,
            all: sortedList,
          });
        }
      }
    } else {
      setGroup(data?.global);
    }
  }, [
    data?.groups,
    proxyGroup.selector,
    getCurrentMode,
    proxyGroupSort,
    setGroup,
  ]);

  useEffect(() => {
    sortGroup();
  }, [sortGroup]);

  const { column } = useBreakpoint({
    sm: 1,
    md: 1,
    lg: 2,
    xl: 3,
    default: 4,
  });

  const [renderList, setRenderList] = useState<RenderClashProxy[][]>([]);

  const updateRenderList = () => {
    if (!group?.all) return;

    const nodeNames: string[] = [];

    const list = group?.all?.reduce<RenderClashProxy[][]>(
      (result, value, index) => {
        const getKey = () => {
          const filter = nodeNames.filter((i) => i === value.name);

          if (filter.length === 0) {
            return value.name;
          } else {
            return `${value.name}-${filter.length}`;
          }
        };

        if (index % column === 0) {
          result.push([]);
        }

        result[Math.floor(index / column)].push({
          ...value,
          renderLayoutKey: getKey(),
        });

        nodeNames.push(value.name);

        return result;
      },
      [],
    );

    setRenderList(list);
  };

  useEffect(() => {
    startTransition(() => {
      updateRenderList();
    });
  }, [group?.all, column]);

  const hendleClick = (node: string) => {
    if (!getCurrentMode.global) {
      setGroupProxy(proxyGroup.selector as number, node);
    } else {
      setGlobalProxy(node);
    }
  };

  const { nyanpasuConfig } = useNyanpasu();

  const disableMotion = nyanpasuConfig?.lighten_animation_effects;

  const vListRef = useRef<VListHandle>(null);

  useImperativeHandle(ref, () => ({
    scrollToCurrent: () => {
      const index = renderList.findIndex((node) =>
        node.some((item) => item.name === group?.now),
      );

      vListRef.current?.scrollToIndex(index, {
        align: "center",
        smooth: true,
      });
    },
  }));

  return (
    <AnimatePresence initial={false} mode="sync">
      <VList
        ref={vListRef}
        style={{ flex: 1 }}
        className={classNames(
          "transition-opacity",
          "p-2",
          isPending ? "opacity-0" : "opacity-1",
        )}
      >
        {renderList?.map((node, index) => {
          return (
            <div
              key={index}
              className="grid gap-2 pb-2"
              style={{ gridTemplateColumns: `repeat(${column} , 1fr)` }}
            >
              {node.map((render) => {
                const Card = () => (
                  <NodeCard
                    node={render}
                    now={group?.now}
                    disabled={group?.type !== "Selector"}
                    onClick={() => hendleClick(render.name)}
                    onClickDelay={async () => {
                      await updateProxiesDelay(render.name);
                    }}
                  />
                );

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
                    initial={{ scale: 0.7, opacity: 0 }}
                    animate={{ scale: 1, opacity: 1 }}
                    exit={{ opacity: 0 }}
                  >
                    <Card />
                  </motion.div>
                );
              })}
            </div>
          );
        })}
      </VList>
    </AnimatePresence>
  );
});
