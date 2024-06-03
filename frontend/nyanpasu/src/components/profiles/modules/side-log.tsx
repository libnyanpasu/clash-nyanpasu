import { RamenDining, Terminal } from "@mui/icons-material";
import { Divider } from "@mui/material";
import { useClash } from "@nyanpasu/interface";
import { memo } from "react";
import { isEmpty } from "lodash-es";
import { VList } from "virtua";
import { filterProfiles } from "../utils";
import { classNames } from "@/utils";

const LogListItem = memo(function LogListItem({
  name,
  item,
  showDivider,
}: {
  name?: string;
  item?: [string, string];
  showDivider?: boolean;
}) {
  return (
    <>
      {showDivider && <Divider />}

      <div className="w-full font-mono break-all">
        <span className="text-red-500">[{name}]: </span>
        <span>{item}</span>
      </div>
    </>
  );
});

export interface SideLogProps {
  className?: string;
}

export const SideLog = ({ className }: SideLogProps) => {
  const { getRuntimeLogs, getProfiles } = useClash();

  const { scripts } = filterProfiles(getProfiles.data?.items);

  return (
    <div className={classNames("w-full", className)}>
      <div className="p-2 pl-4 flex justify-between items-center">
        <div className="flex items-center gap-2">
          <Terminal />

          <span>Console</span>
        </div>
      </div>

      <Divider />

      <VList className="flex flex-col gap-2 p-2 overflow-auto select-text">
        {!isEmpty(getRuntimeLogs.data) ? (
          Object.entries(getRuntimeLogs.data).map(([uid, content]) => {
            return content.map((item, index) => {
              const name = scripts?.find((script) => script.uid === uid)?.name;

              return (
                <LogListItem
                  key={uid + index}
                  name={name}
                  item={item}
                  showDivider={index !== 0}
                />
              );
            });
          })
        ) : (
          <div className="w-full h-full min-h-48 flex flex-col justify-center items-center">
            <RamenDining className="!size-10" />
            <p>No Log</p>
          </div>
        )}
      </VList>
    </div>
  );
};
