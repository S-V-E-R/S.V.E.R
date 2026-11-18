"use client";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useRef, useState } from "react";
import { send } from "../lib/client-api";
import type { Viewer } from "../lib/types";
import { AlertBell } from "./Notifications";
import { ReportForm, TakeDownLink } from "./Report";
import { ShareButton } from "./ShareButton";
import { Subscribe } from "./Subscribe";

/** Follow / Following, the Block and Report menu, or Edit profile and Download card (the share card, app/[username]/opengraph-image.tsx) on your own channel; Share for everyone. */
export function ChannelActions({ username, displayName, viewer, path, credit = null }: { username: string; displayName: string; viewer: Viewer; path: string; credit?: string | null }) {
  const router = useRouter();
  const [following, setFollowing] = useState(viewer.following);
  const [blocked, setBlocked] = useState(viewer.blocked);
  const [menu, setMenu] = useState(false);
  const more = useRef<HTMLButtonElement>(null);
  const [reporting, setReporting] = useState(false);
  const [error, setError] = useState("");
  if (viewer.is_owner) return <div className="channel-actions"><Link className="button small" href="/settings/profile">Edit profile</Link><Link className="button small quiet" href="/studio/channel">Creator Studio</Link><ShareButton path={`/${username}`} title={`${displayName} (@${username}) on S.V.E.R`} credit={credit} canonical networks /><a className="button small quiet" href={`/${username}/opengraph-image`} download={`${username}-sver.png`}>Download card</a></div>;
  if (!viewer.signed_in) return <div className="channel-actions"><Link className="button small" href="/login" data-from={path}>Log in to follow</Link><ShareButton path={`/${username}`} title={`${displayName} (@${username}) on S.V.E.R`} credit={credit} canonical networks /><TakeDownLink target={{ target_type: "profile", target_id: username }} /></div>;
  async function follow() {
    const result = await send<{ following: boolean }>(following ? "DELETE" : "PUT", `/api/follows/${encodeURIComponent(username)}`);
    if (result.ok) { setFollowing(result.data.following); setError(""); router.refresh(); } else setError(result.error);
  }
  async function block() {
    if (!blocked && !window.confirm(`Block @${username}? You'll stop following each other and they can't interact with you. They won't be told.`)) return;
    const result = await send(blocked ? "DELETE" : "PUT", `/api/blocks/${encodeURIComponent(username)}`);
    if (result.ok) { setBlocked(!blocked); setFollowing(false); setMenu(false); router.refresh(); } else setError(result.error);
  }
  return <div className="channel-actions">
    {!blocked && !viewer.interaction_blocked && <button type="button" className={following ? "small quiet" : "small"} aria-pressed={following} onClick={follow}>{following ? "Following" : "Follow"}</button>}
    {following && !blocked && <AlertBell username={username} initial={viewer.following ? viewer.alerts !== false : true} />}
    {!blocked && viewer.can_message && <Link className="button small quiet" href={`/messages?with=${encodeURIComponent(username)}`}>Message</Link>}
    {!blocked && !viewer.interaction_blocked && <Subscribe username={username} />}
    <ShareButton path={`/${username}`} title={`${displayName} (@${username}) on S.V.E.R`} credit={credit} canonical networks />
    <div className="menu" onKeyDown={e => { if (e.key === "Escape" && menu) { setMenu(false); more.current?.focus(); } }}>
      <button ref={more} type="button" className="small quiet" aria-expanded={menu} onClick={() => setMenu(!menu)}>More <span aria-hidden="true">▾</span></button>
      {menu && <div className="menu-list panel">
        <button type="button" className="link-button" onClick={block}>{blocked ? "Unblock" : "Block"}</button>
        <button type="button" className="link-button" onClick={() => { setReporting(true); setMenu(false); }}>Report</button>
      </div>}
    </div>
    {reporting && <div className="panel inline-panel"><ReportForm target={{ target_type: "profile", target_id: username }} onDone={() => setReporting(false)} /></div>}
    {error && <p role="alert" className="form-message">{error}</p>}
  </div>;
}
