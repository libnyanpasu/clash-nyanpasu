import { useDebounceEffect } from "ahooks";
import { useRef } from "react";
import { VList, VListHandle } from "virtua";
import { LogMessage } from "@nyanpasu/interface";
import LogItem from "./log-item";

export const LogList = ({ data }: { data: LogMessage[] }) => {
  const vListRef = useRef<VListHandle>(null);

  const shouldStickToBottom = useRef(true);

  useDebounceEffect(
    () => {
      if (shouldStickToBottom.current) {
        vListRef.current?.scrollToIndex(data.length - 1, {
          align: "end",
          smooth: true,
        });
      }
    },
    [data],
    { wait: 100 },
  );

  return (
    <VList
      ref={vListRef}
      className="flex min-h-full select-text flex-col gap-2 overflow-auto p-2"
      reverse
    >
      {data.map((item, index) => {
        return <LogItem key={index} value={item} />;
      })}
    </VList>
  );
};
