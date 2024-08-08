import { useEffect, useRef, useState } from "react";
import { cn } from "@/utils";

export interface LazyImageProps
  extends React.ImgHTMLAttributes<HTMLImageElement> {
  loadingClassName?: string;
}
export default function LazyImage(props: LazyImageProps) {
  const [loading, setLoading] = useState(true);
  const imgRef = useRef<HTMLImageElement>(null);
  useEffect(() => {
    if (imgRef.current) {
      imgRef.current.onload = () => setLoading(false);
    }
  }, [props.src]);

  return (
    <>
      <div
        className={cn(
          "inline-block animate-pulse bg-slate-100 ring-1 ring-white dark:bg-slate-700 dark:ring-slate-700",
          props.className,
          props.loadingClassName,
          loading && "block",
        )}
      />
      <img
        {...props}
        loading="lazy"
        ref={imgRef}
        className={cn(props.className, !loading && "block")}
      />
    </>
  );
}
