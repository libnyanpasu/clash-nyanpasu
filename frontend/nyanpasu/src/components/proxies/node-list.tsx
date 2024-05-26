import {
  Box,
  Chip,
  ChipProps,
  CircularProgress,
  useTheme,
} from "@mui/material";
import { PaperSwitchButton } from "../setting/modules/system-proxy";
import { Clash, useClashCore, useNyanpasu } from "@nyanpasu/interface";
import { useBreakpoint } from "@nyanpasu/ui";
import { useAtom, useAtomValue } from "jotai";
import { proxyGroupAtom, proxyGroupSortAtom } from "@/store";
import { CSSProperties, memo, useEffect, useMemo, useState } from "react";
import { classNames } from "@/utils";
import { VList } from "virtua";
import { AnimatePresence, motion } from "framer-motion";

type History = Clash.Proxy["history"];

type RenderClashProxy = Clash.Proxy<string> & { renderLayoutKey: string };

const filterDelay = (history?: History): number => {
  if (!history || history.length == 0) {
    return 0;
  } else {
    return history[history.length - 1].delay;
  }
};

const getColorForDelay = (delay: number): string => {
  const { palette } = useTheme();

  const delayColorMapping: { [key: string]: string } = {
    "0": palette.text.secondary,
    "100": palette.success.main,
    "500": palette.warning.main,
    "1000": palette.error.main,
  };

  let color: string = palette.text.secondary;

  for (const key in delayColorMapping) {
    if (delay <= parseInt(key)) {
      color = delayColorMapping[key];
      break;
    }
  }

  return color;
};

const FeatureChip = memo(function FeatureChip(props: ChipProps) {
  return (
    <Chip
      variant="outlined"
      size="small"
      {...props}
      sx={{
        fontSize: 10,
        height: 16,
        padding: 0,

        "& .MuiChip-label": {
          padding: "0 4px",
        },
        ...props.sx,
      }}
    />
  );
});

const DelayChip = memo(function DelayChip({
  delay,
  onClick,
}: {
  delay: number;
  onClick: () => Promise<void>;
}) {
  const [loading, setLoading] = useState(false);

  const handleClick = async () => {
    try {
      setLoading(true);

      await onClick();
    } finally {
      setLoading(false);
    }
  };

  return (
    <FeatureChip
      sx={{
        ml: "auto",
        color: getColorForDelay(delay),
      }}
      label={
        <>
          <span
            className={classNames(
              "transition-opacity",
              loading ? "opacity-0" : "opacity-1",
            )}
          >
            {`${delay} ms`}
          </span>

          <CircularProgress
            size={12}
            className={classNames(
              "transition-opacity",
              "absolute",
              "animate-spin",
              "top-0",
              "bottom-0",
              "left-0",
              "right-0",
              "m-auto",
              loading ? "opacity-1" : "opacity-0",
            )}
          />
        </>
      }
      variant="filled"
      onClick={(e) => {
        e.preventDefault();
        e.stopPropagation();
        handleClick();
      }}
    />
  );
});

const NodeCard = memo(function NodeCard({
  node,
  now,
  disabled,
  onClick,
  onClickDelay,
  style,
}: {
  node: Clash.Proxy<string>;
  now?: string;
  disabled?: boolean;
  onClick: () => void;
  onClickDelay: () => Promise<void>;
  style?: CSSProperties;
}) {
  const delay = useMemo(() => filterDelay(node.history), [node.history]);

  return (
    <PaperSwitchButton
      label={node.name}
      checked={node.name === now}
      onClick={onClick}
      disabled={disabled}
      style={style}
    >
      <Box width="100%" display="flex" gap={0.5}>
        <FeatureChip label={node.type} />

        {node.udp && <FeatureChip label="UDP" />}

        <DelayChip delay={delay} onClick={onClickDelay} />
      </Box>
    </PaperSwitchButton>
  );
});

export const NodeList = () => {
  const { data, setGroupProxy, updateProxiesDelay } = useClashCore();

  const { getCurrentMode } = useNyanpasu();

  const [proxyGroup] = useAtom(proxyGroupAtom);

  const proxyGroupSort = useAtomValue(proxyGroupSortAtom);

  const [group, setGroup] = useState<Clash.Proxy<Clash.Proxy<string>>>();

  useEffect(() => {
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
  }, [data?.groups, proxyGroup.selector, getCurrentMode, proxyGroupSort]);

  const { column } = useBreakpoint({
    sm: 1,
    md: 1,
    lg: 2,
    xl: 3,
    default: 4,
  });

  const [renderList, setRenderList] = useState<RenderClashProxy[][]>([]);

  useEffect(() => {
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
  }, [group?.all, column]);

  const hendleClick = (node: string) => {
    setGroupProxy(proxyGroup.selector as number, node);
  };

  return (
    <AnimatePresence initial={false}>
      <VList style={{ flex: 1 }} className="p-2">
        {renderList?.map((node, index) => {
          return (
            <div
              key={index}
              className="grid gap-2 pb-2"
              style={{ gridTemplateColumns: `repeat(${column} , 1fr)` }}
            >
              {node.map((render) => {
                return (
                  <motion.div
                    key={render.name}
                    layoutId={`node-${render.renderLayoutKey}`}
                    className="relative overflow-hidden"
                    layout="position"
                    initial={false}
                    animate="center"
                    exit="exit"
                  >
                    <NodeCard
                      node={render}
                      now={group?.now}
                      disabled={group?.type !== "Selector"}
                      onClick={() => hendleClick(render.name)}
                      onClickDelay={async () => {
                        await updateProxiesDelay(render.name);
                      }}
                    />
                  </motion.div>
                );
              })}
            </div>
          );
        })}
      </VList>
    </AnimatePresence>
  );
};
