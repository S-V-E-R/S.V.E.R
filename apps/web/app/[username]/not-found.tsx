import Link from "next/link";

// Shared channel 404 for unknown, internal, deleted, held and restricted channels alike.
export default function ChannelNotFound() {
  return <div className="channel"><section className="panel">
    <span className="eyebrow">CHANNEL</span>
    <h1>{"This channel doesn't exist."}</h1>
    <p className="intro">Check the username and try again.</p>
    <Link href="/">Back to S.V.E.R</Link>
  </section></div>;
}
