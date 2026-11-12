import type { Metadata } from "next";

export const metadata: Metadata = { title: "Connect an app | S.V.E.R", robots: { index: false, follow: false } };

export default function GoLayout({ children }: { children: React.ReactNode }) {
  return children;
}
