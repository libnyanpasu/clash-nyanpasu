import { useState } from "react";
import { cn } from "@/utils";

export interface LazyImageProps
  extends React.ImgHTMLAttributes<HTMLImageElement> {
  loadingClassName?: string;
}
export function LazyImage(props: LazyImageProps) {
  const [loading, setLoading] = useState(true);

  return (
    <>
      <div
        className={cn(
          "inline-block animate-pulse bg-slate-200 ring-1 ring-slate-200 dark:bg-slate-700 dark:ring-slate-700",
          props.className,
          props.loadingClassName,
          loading ? "inline-block" : "hidden",
        )}
      />
      <img
        {...props}
        onLoad={() => setLoading(false)}
        className={cn(props.className, loading ? "hidden" : "inline-block")}
      />
    </>
  );
}
