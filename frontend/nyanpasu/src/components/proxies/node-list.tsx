import { useMemoizedFn } from "ahooks";
import { AnimatePresence, motion } from "framer-motion";
import { useAtom, useAtomValue } from "jotai";
import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
  useTransition,
} from "react";
import { VList, VListHandle } from "virtua";
import { proxyGroupAtom, proxyGroupSortAtom } from "@/store";
import { classNames } from "@/utils";
import { Clash, useClashCore, useNyanpasu } from "@nyanpasu/interface";
import { useBreakpoint } from "@nyanpasu/ui";
import NodeCard from "./node-card";
import { nodeSortingFn } from "./utils";

type RenderClashProxy = Clash.Proxy<string> & { renderLayoutKey: string };

export interface NodeListRef {
  scrollToCurrent: () => void;
}

export const NodeList = forwardRef(function NodeList({}, ref) {
  const {
    data,
    setGroupProxy,
    setGlobalProxy,
    updateProxiesDelay,
    getAllProxiesProviders,
  } = useClashCore();

  const [isPending, startTransition] = useTransition();

  const { getCurrentMode } = useNyanpasu();

  const [proxyGroup] = useAtom(proxyGroupAtom);

  const proxyGroupSort = useAtomValue(proxyGroupSortAtom);

  const [group, setGroup] = useState<Clash.Proxy<Clash.Proxy<string>>>();

  const sortGroup = useMemoizedFn(() => {
    if (!getCurrentMode.global) {
      if (proxyGroup.selector !== null) {
        const selectedGroup = data?.groups[proxyGroup.selector];

        if (selectedGroup) {
          setGroup(nodeSortingFn(selectedGroup, proxyGroupSort));
        }
      }
    } else {
      if (data?.global) {
        setGroup(nodeSortingFn(data?.global, proxyGroupSort));
      } else {
        setGroup(data?.global);
      }
    }
  });

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

  const updateRenderList = useMemoizedFn(() => {
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
  });

  useEffect(() => {
    startTransition(() => {
      updateRenderList();
    });
  }, [group?.all, column, updateRenderList]);

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

  const handleClickDelay = async (name: string) => {
    const getGroupTestUrl = () => {
      if (group?.name) {
        return getAllProxiesProviders.data?.[group?.name].testUrl;
      }
    };

    await updateProxiesDelay(name, {
      url: getGroupTestUrl(),
    });
  };

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
                    onClickDelay={async () =>
                      await handleClickDelay(render.name)
                    }
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
                    initial={false}
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
