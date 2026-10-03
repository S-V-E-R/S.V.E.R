import Link from "next/link";
import { redirect } from "next/navigation";
import { FollowingList } from "../../components/FollowingList";
import type { PeoplePage } from "../../components/People";
import { apiGet } from "../../lib/server-api";
import { currentAccount } from "../session";
import "../../styles/profiles.css";

export const metadata = { title: "Following | S.V.E.R", robots: { index: false, follow: false } };
export default async function Following({ searchParams }: { searchParams: Promise<{ cursor?: string }> }) {
  if (!(await currentAccount())) redirect("/login");
  const cursor = (await searchParams).cursor;
  const page = (await apiGet<PeoplePage>(`/api/me/following${cursor ? `?cursor=${encodeURIComponent(cursor)}` : ""}`)).data;
  return <div className="settings-page single"><div className="settings-body">
    <h1>Following</h1>
    <section className="panel section">{cursor && <Link href="/following">Back to start</Link>}<FollowingList page={page} /></section>
  </div></div>;
}
