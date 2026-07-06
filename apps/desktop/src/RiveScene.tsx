/// <reference types="vite/client" />
import { lazy, Suspense, type ReactNode } from "react";

/* Rive + code blended: when a .riv asset exists in
   src/assets/rive/, it renders via the Rive runtime; until then the code-driven
   scene (children) shows. Drop authored files in and they take over — zero code
   changes. The runtime chunk only loads when an asset is actually present. */
const assets = import.meta.glob("./assets/rive/*.riv", { eager: false, query: "?url", import: "default" });

const RiveRuntime = lazy(async () => {
  const mod = await import("@rive-app/react-canvas");
  function Player({ src }: { src: string }) {
    const { RiveComponent } = mod.useRive({ src, autoplay: true });
    return <RiveComponent style={{ width: "100%", height: "100%" }} />;
  }
  return { default: Player };
});

export function RiveScene({ name, children }: { name: string; children: ReactNode }) {
  const loader = assets[`./assets/rive/${name}`];
  if (!loader) return <>{children}</>;
  return (
    <Suspense fallback={<>{children}</>}>
      <RiveUrl loader={loader as () => Promise<string>} fallback={children} />
    </Suspense>
  );
}

import { useEffect, useState } from "react";
function RiveUrl({ loader, fallback }: { loader: () => Promise<string>; fallback: ReactNode }) {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    loader().then(setUrl).catch(() => setUrl(null));
  }, [loader]);
  if (!url) return <>{fallback}</>;
  return <RiveRuntime src={url} />;
}
