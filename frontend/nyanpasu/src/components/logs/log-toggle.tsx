import { atomEnableLog } from "@/store";
import { IconButton } from "@mui/material";
import { useAtom } from "jotai";
import {
  PauseCircleOutlineRounded,
  PlayCircleOutlineRounded,
} from "@mui/icons-material";

export const LogToggle = () => {
  const [enableLog, setEnableLog] = useAtom(atomEnableLog);

  return (
    <IconButton
      size="small"
      color="inherit"
      onClick={() => setEnableLog((e) => !e)}
    >
      {enableLog ? <PauseCircleOutlineRounded /> : <PlayCircleOutlineRounded />}
    </IconButton>
  );
};

export default LogToggle;
