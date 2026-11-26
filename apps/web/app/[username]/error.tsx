"use client";
import Link from "next/link";

// A channel that couldn't be loaded (the API was slow or down). Not the 404: the channel may be fine.
export default function ChannelError() {
  return <div className="channel"><section className="panel">
    <span className="eyebrow">CHANNEL</span>
    <h1>{"We couldn't load this channel."}</h1>
    <p className="intro">Something went wrong on our side. Please try again in a moment.</p>
    <p><button type="button" className="button" onClick={() => window.location.reload()}>Try again</button> <Link href="/">Back to S.V.E.R</Link></p>
  </section></div>;
}
