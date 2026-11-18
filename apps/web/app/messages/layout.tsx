import type { Metadata } from "next";
import { redirect } from "next/navigation";
import { currentAccount } from "../session";

export const metadata: Metadata = { title: "Messages | S.V.E.R", robots: { index: false, follow: false } };

export default async function MessagesLayout({ children }: { children: React.ReactNode }) {
  if (!(await currentAccount())) redirect("/login?next=/messages");
  return children;
}
