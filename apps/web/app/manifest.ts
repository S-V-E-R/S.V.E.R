import type { MetadataRoute } from "next";

// Web app manifest, served at /manifest.webmanifest. Icons are the legacy S.V.E.R logo;
// they live under /icons, a reserved name. Name and description carried over from the legacy site.
export default function manifest(): MetadataRoute.Manifest {
  return {
    name: "S.V.E.R - Streaming Vigorously Ensures Revenue",
    short_name: "S.V.E.R",
    description: "Join the warfront. Choose your faction. Stream with purpose.",
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
