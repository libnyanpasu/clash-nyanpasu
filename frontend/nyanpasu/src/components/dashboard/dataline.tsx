import parseTraffic from "@/utils/parse-traffic";
import { SvgIconComponent } from "@mui/icons-material";
import { Paper } from "@mui/material";
import { Sparkline } from "@nyanpasu/ui";
import { FC, cloneElement } from "react";
import { useTranslation } from "react-i18next";

export interface DatalineProps {
  data: number[];
  icon: SvgIconComponent;
  title: string;
  total?: number;
  type?: "speed" | "raw";
}

export const Dataline: FC<DatalineProps> = ({
  data,
  icon,
  title,
  total,
  type,
}) => {
  const { t } = useTranslation();

  return (
    <Paper className="!rounded-3xl relative">
      <Sparkline data={data} className="rounded-3xl" />

      <div className="absolute top-0 p-4 h-full flex flex-col gap-4 justify-between">
        <div className="flex items-center gap-2">
          {cloneElement(icon)}

          <div className="font-bold">{title}</div>
        </div>

        <div className="font-bold text-2xl text-shadow-md">
          {type === "raw" ? data.at(-1) : parseTraffic(data.at(-1)).join(" ")}
          {type === "speed" && "/s"}
        </div>

        <div className=" h-5">
          {total && (
            <span className="text-shadow-sm">
              {t("Total")}: {parseTraffic(total).join(" ")}
            </span>
          )}
        </div>
      </div>
    </Paper>
  );
};

export default Dataline;
