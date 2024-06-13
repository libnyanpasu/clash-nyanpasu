import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import { useNavigate } from "react-router-dom";

export const SchemeProvider = () => {
  const navigate = useNavigate();

  useEffect(() => {
    listen("scheme-request-received", (req) => {
      const message: string = req.payload as string;

      const url = new URL(message);

      if (url.pathname.endsWith("/")) {
        url.pathname = url.pathname.slice(0, -1);
      }

      if (url.pathname.startsWith("//")) {
        url.pathname = url.pathname.slice(1);
      }

      switch (url.pathname) {
        case "/subscribe-remote-profile":
          navigate("/profiles", {
            state: {
              subscribe: {
                url: url.searchParams.get("url"),
                name: url.searchParams.has("name")
                  ? decodeURIComponent(url.searchParams.get("name")!)
                  : undefined,
                desc: url.searchParams.has("desc")
                  ? decodeURIComponent(url.searchParams.get("desc")!)
                  : undefined,
              },
            },
          });
      }
    });
  }, []);

  return null;
};

export default SchemeProvider;
