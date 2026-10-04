"use client";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useState } from "react";
import { send } from "../lib/client-api";
import type { Viewer } from "../lib/types";
import { AlertBell } from "./Notifications";
import { ReportForm, TakeDownLink } from "./Report";
import { ShareButton } from "./ShareButton";

/** Follow / Following, the Block and Report menu, or Edit profile on your own channel; Share for everyone. */
export function ChannelActions({ username, displayName, viewer, path }: { username: string; displayName: string; viewer: Viewer; path: string }) {
  const router = useRouter();
  const [following, setFollowing] = useState(viewer.following);
  const [blocked, setBlocked] = useState(viewer.blocked);
  const [menu, setMenu] = useState(false);
  const [reporting, setReporting] = useState(false);
  const [error, setError] = useState("");
  if (viewer.is_owner) return <div className="channel-actions"><Link className="button small" href="/settings/profile">Edit profile</Link><Link className="button small quiet" href="/studio/channel">Creator Studio</Link><ShareButton username={username} displayName={displayName} /><TakeDownLink target={{ target_type: "profile", target_id: username }} /></div>;
  if (!viewer.signed_in) return <div className="channel-actions"><Link className="button small" href="/login" data-from={path}>Log in to follow</Link><ShareButton username={username} displayName={displayName} /><TakeDownLink target={{ target_type: "profile", target_id: username }} /></div>;
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
    <ShareButton username={username} displayName={displayName} />
    <div className="menu">
      <button type="button" className="small quiet" aria-haspopup="true" aria-expanded={menu} onClick={() => setMenu(!menu)}>More <span aria-hidden="true">▾</span></button>
      {menu && <div className="menu-list panel" role="menu">
        <button type="button" role="menuitem" className="link-button" onClick={block}>{blocked ? "Unblock" : "Block"}</button>
        <button type="button" role="menuitem" className="link-button" onClick={() => { setReporting(true); setMenu(false); }}>Report</button>
      </div>}
    </div>
    {reporting && <div className="panel inline-panel"><ReportForm target={{ target_type: "profile", target_id: username }} onDone={() => setReporting(false)} /></div>}
    {error && <p role="alert" className="form-message">{error}</p>}
  </div>;
}
