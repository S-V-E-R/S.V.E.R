import type { MetadataRoute } from "next";

// Web app manifest, served at /manifest.webmanifest. Icons are the legacy S.V.E.R logo;
// they live under /icons, a reserved name.
export default function manifest(): MetadataRoute.Manifest {
  return {
    name: "S.V.E.R",
    short_name: "S.V.E.R",
    description: "Live streaming for people who play, build, and make.",
    start_url: "/",
    display: "browser",
    background_color: "#07080B",
    theme_color: "#07080B",
    icons: [
      { src: "/icons/icon-192.png", sizes: "192x192", type: "image/png", purpose: "any" },
      { src: "/icons/icon-512.png", sizes: "512x512", type: "image/png", purpose: "any" },
      { src: "/icons/icon-maskable-512.png", sizes: "512x512", type: "image/png", purpose: "maskable" },
    ],
  };
}
