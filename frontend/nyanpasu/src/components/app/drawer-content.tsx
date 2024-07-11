import getSystem from "@/utils/get-system";
import clsx from "clsx";
import AnimatedLogo from "../layout/animated-logo";
import { getRoutesWithIcon } from "@/utils/routes-utils";
import RouteListItem from "./modules/route-list-item";
import { useCallback, useEffect, useRef, useState } from "react";
import { useSize } from "ahooks";
import { languageQuirks } from "@/utils/language";
import { useNyanpasu } from "@nyanpasu/interface";

export const DrawerContent = ({ className }: { className?: string }) => {
  const [onlyIcon, setOnlyIcon] = useState(false);

  const { nyanpasuConfig } = useNyanpasu();

  const routes = getRoutesWithIcon();

  const contentRef = useRef<HTMLDivElement | null>(null);

  const size = useSize(contentRef.current);

  const handleResize = useCallback(
    (value?: number) => {
      if (value) {
        if (
          value <
          languageQuirks[nyanpasuConfig?.language ?? "en"].drawer.minWidth
        ) {
          setOnlyIcon(true);
        } else {
          setOnlyIcon(false);
        }
      } else {
        setOnlyIcon(false);
      }
    },
    [nyanpasuConfig?.language],
  );

  useEffect(() => {
    handleResize(size?.width);
  }, [size?.width]);

  return (
    <div
      ref={contentRef}
      className={clsx(
        "p-4",
        getSystem() === "macos" ? "pt-14" : "pt-8",
        "w-full",
        "h-full",
        "flex",
        "flex-col",
        "gap-4",
        className,
      )}
      style={{
        backgroundColor: "var(--background-color-alpha)",
      }}
      data-windrag
    >
      <div className="flex items-center justify-center gap-4 mx-2">
        <div className=" h-full max-w-28 max-h-28" data-windrag>
          <AnimatedLogo className="w-full h-full" data-windrag />
        </div>

        {!onlyIcon && (
          <div
            className="text-lg font-bold mt-1 mr-1 whitespace-pre-wrap"
            data-windrag
          >
            {"Clash\nNyanpasu"}
          </div>
        )}
      </div>

      <div className="flex flex-col gap-2 overflow-y-auto scrollbar-hidden !overflow-x-hidden">
        {Object.entries(routes).map(([name, { path, icon }]) => {
          return (
            <RouteListItem
              key={name}
              name={name}
              path={path}
              icon={icon}
              onlyIcon={onlyIcon}
            />
          );
        })}
      </div>
    </div>
  );
};

export default DrawerContent;
