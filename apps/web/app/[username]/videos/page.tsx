import Link from "next/link";
import { ChannelFrame } from "../../../components/ChannelFrame";
import { VideoGrid } from "../../../components/VideoGrid";
import { loadChannel, channelMetadata, type ChannelParams } from "../../../lib/channel";
import { apiGet } from "../../../lib/server-api";
import type { VideoCard } from "../../../lib/videos";
export async function generateMetadata({ params }: { params: ChannelParams }) { return channelMetadata((await params).username, "Videos"); }
export default async function Videos({ params, searchParams }: { params: ChannelParams; searchParams: Promise<{ sort?: string; offset?: string }> }) {
  const data = await loadChannel((await params).username);
  const username = data.channel.username;
  const query = await searchParams;
  const popular = query.sort === "popular";
  const offset = /^\d{1,7}$/.test(query.offset ?? "") ? Math.min(1000000, Number(query.offset)) : 0;
  const result = await apiGet<{ videos: VideoCard[]; has_more: boolean }>(`/api/channels/${encodeURIComponent(username)}/videos?popular=${popular}&offset=${offset}`);
  return <ChannelFrame data={data} path={`/${username}/videos`}><h2>Videos</h2><nav className="video-actions" aria-label="Video order"><Link href={`/${username}/videos`} aria-current={!popular ? "page" : undefined}>Newest</Link><Link href={`/${username}/videos?sort=popular`} aria-current={popular ? "page" : undefined}>Most viewed</Link></nav>
    {!result.data ? <p role="alert">Videos could not be loaded. Please refresh.</p> : ([['HIGHLIGHT', 'Highlights'], ['VOD', 'Past broadcasts'], ['CLIP', 'Clips']] as const).map(([kind, label]) => <section key={kind}><h2>{label}</h2>{result.data!.videos.some(v => v.video.kind === kind) ? <VideoGrid items={result.data!.videos.filter(v => v.video.kind === kind)} /> : <p className="muted">No {label.toLowerCase()} to show yet.</p>}</section>)}
    <nav className="video-actions" aria-label="Video pages">{offset > 0 && <Link href={`/${username}/videos?sort=${popular ? "popular" : "newest"}&offset=${Math.max(0,offset-20)}`}>Previous videos</Link>}{result.data?.has_more && <Link href={`/${username}/videos?sort=${popular ? "popular" : "newest"}&offset=${offset+20}`}>More videos</Link>}</nav>
  </ChannelFrame>;
}
