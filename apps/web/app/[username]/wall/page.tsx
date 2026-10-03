import Link from "next/link";
import { ChannelFrame } from "../../../components/ChannelFrame";
import { WallComposer, WallPost } from "../../../components/Wall";
import { channelMetadata, loadChannel, type ChannelParams } from "../../../lib/channel";
import { apiGet } from "../../../lib/server-api";
import type { Post, WallViewer } from "../../../lib/types";

type Page = { pinned: Post[]; items: Post[]; next_cursor: string | null; viewer: WallViewer };
export async function generateMetadata({ params }: { params: ChannelParams }) {
  return channelMetadata((await params).username, "Wall");
}
export default async function WallTab({ params, searchParams }: { params: ChannelParams; searchParams: Promise<{ cursor?: string }> }) {
  const data = await loadChannel((await params).username);
  const name = data.channel.username;
  const cursor = (await searchParams).cursor;
  const page = (await apiGet<Page>(`/api/channels/${encodeURIComponent(name)}/wall${cursor ? `?cursor=${encodeURIComponent(cursor)}` : ""}`)).data;
  return <ChannelFrame data={data} path={`/${name}/wall`}>
    <section className="section">
      <h2>Wall</h2>
      {page && <WallComposer username={name} viewer={page.viewer} />}
      {!page ? <p className="muted">The wall couldn&apos;t be loaded. Please try again.</p> : <>
        {[...page.pinned, ...page.items].map(p => <WallPost key={p.id} post={p} viewer={page.viewer} />)}
        {page.pinned.length + page.items.length === 0 && <p className="muted">No posts yet.</p>}
        <nav className="pager">{cursor && <Link href={`/${name}/wall`}>Newest</Link>}{page.next_cursor && <Link href={`/${name}/wall?cursor=${encodeURIComponent(page.next_cursor)}`}>Older posts</Link>}</nav>
      </>}
    </section>
  </ChannelFrame>;
}
