import { useDebounceEffect } from "ahooks";
import { RefObject, useRef } from "react";
import { Virtualizer, VListHandle } from "virtua";
import { LogMessage } from "@nyanpasu/interface";
import LogItem from "./log-item";

export interface LogListProps {
  data: LogMessage[];
  scrollRef: RefObject<HTMLElement>;
}

export const LogList = ({ data, scrollRef }: LogListProps) => {
  const virtualizerRef = useRef<VListHandle>(null);

  const shouldStickToBottom = useRef(true);

  const isFristScroll = useRef(true);

  useDebounceEffect(
    () => {
      if (shouldStickToBottom) {
        virtualizerRef.current?.scrollToIndex(data.length - 1, {
          align: "end",
          smooth: !isFristScroll.current,
        });

        isFristScroll.current = false;
      }
    },
    [data],
    { wait: 100 },
  );

  const handleRangeChange = (_start: number, end: number) => {
    if (end + 1 === data.length) {
      shouldStickToBottom.current = true;
    } else {
      shouldStickToBottom.current = false;
    }
  };

  return (
    <Virtualizer
      ref={virtualizerRef}
      scrollRef={scrollRef}
      onRangeChange={handleRangeChange}
    >
      {data.map((item, index) => {
        return <LogItem key={index} value={item} />;
      })}
    </Virtualizer>
  );
};
