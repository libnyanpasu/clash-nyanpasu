import { atomLogData } from "@/store";
import { Close } from "@mui/icons-material";
import { Tooltip } from "@mui/material";
import { FloatingButton } from "@nyanpasu/ui";
import { useSetAtom } from "jotai";
import { useTranslation } from "react-i18next";

export const ClearLogButton = () => {
  const { t } = useTranslation();

  const setLogData = useSetAtom(atomLogData);

  const onClear = () => {
    setLogData([]);
  };

  return (
    <Tooltip title={t("Clear")}>
      <FloatingButton onClick={onClear}>
        <Close className="!size-8 absolute" />
      </FloatingButton>
    </Tooltip>
  );
};

export default ClearLogButton;
