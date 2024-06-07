import { LogMessage } from "@nyanpasu/interface";
import { useThrottleFn } from "ahooks";
import { useEffect, useRef } from "react";
import { VList, VListHandle } from "virtua";
import LogItem from "./log-item";

export const LogList = ({ data }: { data: LogMessage[] }) => {
  const vListRef = useRef<VListHandle>(null);

  const shouldStickToBottom = useRef(true);

  const { run: scrollToBottom } = useThrottleFn(
    () => {
      if (shouldStickToBottom.current) {
        setTimeout(() => {
          vListRef.current?.scrollToIndex(data.length - 1, {
            align: "end",
            smooth: true,
          });
        }, 100);
      }
    },
    { wait: 100 },
  );

  useEffect(() => {
    scrollToBottom();
  }, [data]);

  return (
    <VList
      ref={vListRef}
      className="flex flex-col gap-2 p-2 overflow-auto select-text min-h-full"
    >
      {data.map((item, index) => {
        return <LogItem key={index} value={item} />;
      })}
    </VList>
  );
};
