import { Clash } from "@nyanpasu/interface";
import { CSSProperties, memo, useMemo } from "react";
import { filterDelay } from "./utils";
import { PaperSwitchButton } from "../setting/modules/system-proxy";
import Box from "@mui/material/Box";
import FeatureChip from "./feature-chip";
import DelayChip from "./delay-chip";

export const NodeCard = memo(function NodeCard({
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

export default NodeCard;
