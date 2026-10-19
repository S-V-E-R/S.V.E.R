import Link from "next/link";
import { Fastmarch } from "../components/Fastmarch";
import { currentAccount } from "./session";

/** Any missing page: the Static took it. Fastmarch while you're here (docs/FASTMARCH.md). */
export default async function NotFound() {
  const account = await currentAccount();
  return <div className="site-page info-page">
    <header className="site-page-heading">
      <div className="eyebrow">S.V.E.R / 404</div>
      <h1>This page was lost to the Static</h1>
      <p className="site-intro">The link may be old, or the page has moved. <Link href="/">Go home</Link> or <Link href="/browse">browse live channels</Link>, or run the signal back out yourself.</p>
    </header>
    <Fastmarch faction={account?.faction ?? null} />
  </div>;
}
