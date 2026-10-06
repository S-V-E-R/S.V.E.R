import type { NextConfig } from "next";
const config: NextConfig = {
  poweredByHeader: false,
  async rewrites() { return [{ source: "/api/:path*", destination: `${process.env.API_INTERNAL_ORIGIN || "http://127.0.0.1:8080"}/api/:path*` }]; },
  async headers() { return [{ source: "/:path*", headers: [
    { key: "Referrer-Policy", value: "no-referrer" },
    { key: "X-Content-Type-Options", value: "nosniff" },
    { key: "Permissions-Policy", value: "camera=(), microphone=(), geolocation=()" }
  ] }, { source: "/((?!embed/).*)", headers: [{ key: "X-Frame-Options", value: "DENY" }] }, { source: "/embed/:id", headers: [{ key: "Cache-Control", value: "no-store" }, { key: "Content-Security-Policy", value: "frame-ancestors *" }] }]; }
};
export default config;
