import type { Metadata } from "next";
import { authPage, type AuthSearchParams } from "../auth-page";

export const metadata: Metadata = { title: "Enlist | S.V.E.R", robots: { index: false, follow: false } };

export default async function Page({ searchParams }: { searchParams: AuthSearchParams }) {
  return authPage("signup", searchParams);
}
