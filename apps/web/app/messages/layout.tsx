import type { Metadata } from "next";

export const metadata: Metadata = { title: "Messages | S.V.E.R", robots: { index: false, follow: false } };

export default function MessagesLayout({ children }: { children: React.ReactNode }) {
  return children;
}
