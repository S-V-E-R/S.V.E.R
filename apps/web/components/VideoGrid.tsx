import Link from "next/link";
import { duration, videoPath, type VideoCard } from "../lib/videos";
import "../styles/videos.css";

export function VideoGrid({ items }: { items: VideoCard[] }) {
  return <div className="video-grid">{items.map(({ video: v, thumbnail, channel }) => <article key={v.id} className="video-card">
    <Link href={videoPath(v)} className="video-poster" aria-label={`Watch ${v.title}`}>
      {/* Signed private thumbnails are delivered by the API, never the public image optimizer. */}
      {/* eslint-disable-next-line @next/next/no-img-element */}
      {thumbnail ? <img src={thumbnail} alt="" loading="lazy" width={640} height={360} /> : <span>{v.mature ? "18+" : v.kind === "HIGHLIGHT" ? "Highlight" : v.kind === "CLIP" ? "Clip" : "Past broadcast"}</span>}
      <span className="video-duration">{duration(v.duration_ms)}</span>
    </Link>
    <h3><Link href={videoPath(v)}>{v.title}</Link></h3>
    {channel?.username && <Link href={`/${channel.username}`}>{channel.display_name}</Link>}
    <p className="muted">{v.category ? `${v.category} · ` : ""}{v.views.toLocaleString()} views · {new Date(v.created_at).toLocaleDateString("en-US", { timeZone: "UTC" })}</p>
  </article>)}</div>;
}
