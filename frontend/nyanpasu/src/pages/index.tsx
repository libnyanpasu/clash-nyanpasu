import { useAtomValue } from "jotai";
import { useEffect } from "react";
import { useNavigate } from "@/router";
import { memorizedRoutePathAtom } from "@/store";

export default function IndexPage() {
  const navigate = useNavigate();
  const memorizedNavigate = useAtomValue(memorizedRoutePathAtom);
  useEffect(() => {
    navigate(
      memorizedNavigate && memorizedNavigate !== "/"
        ? memorizedNavigate
        : "/dashboard",
    );
  }, [memorizedNavigate, navigate]);
  return null;
}
