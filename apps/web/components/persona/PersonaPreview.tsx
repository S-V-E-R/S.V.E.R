"use client";
import { useEffect, useState } from "react";
import { send } from "../../lib/client-api";
import { factionOf } from "../../lib/factions";
import type { Channel } from "../../lib/types";
import { Avatar } from "../Avatar";
import { Crest } from "../Crest";
import { MusicIcon } from "../shell/Icons";

/**
 * How your header looks to visitors, beside the Persona editor and the setup steps. It reloads
 * your public channel whenever a section saves (`version` changes), so it shows what visitors see.
 */
export function PersonaPreview({ username, version }: { username: string; version: number }) {
  const [data, setData] = useState<Channel | null>(null);
  useEffect(() => {
    let stopped = false;
    void send<Channel>("GET", `/api/channels/${encodeURIComponent(username)}`).then(r => { if (!stopped && r.ok) setData(r.data); });
    return () => { stopped = true; };
  }, [username, version]);
  if (!data) return <aside className="persona-preview panel" aria-label="Preview"><p className="muted">Loading the preview…</p></aside>;
  const c = data.channel;
  const banner = c.banner ? Object.values(c.banner) : [];
  const faction = factionOf(c.faction);
  return <aside className="persona-preview" aria-label="Preview of your header" data-theme={faction?.slug ?? "neutral"} data-variant={c.theme_variant ?? undefined}>
    <p className="eyebrow">What visitors see</p>
    <div className="channel-header frame">
      <div className={banner.length ? "channel-banner" : "channel-banner none"}>
        {/* eslint-disable-next-line @next/next/no-img-element */}
        {banner.length > 0 && <img className="banner" src={banner[0]} alt="" />}
      </div>
      <div className="preview-identity">
        <span className="avatar-frame" data-frame={c.frame ?? 0}><Avatar sizes={c.avatar} name={c.display_name} size={64} /></span>
        <div>
          {data.header?.label && <span className="page-label">{data.header.label}</span>}
          <strong className="preview-name">{c.display_name}</strong>
          <p className="handle">@{c.username}{!!c.level && <span className="level-hex">Lv {c.level}{c.title && <> · {c.title}</>}</span>}</p>
          {faction && <p className="faction-line"><Crest faction={faction.slug} initial="" size={18} />{faction.name}</p>}
          {(c.mood_emoji || c.currently || c.status_text) && <p className="status-line">{c.mood_emoji && <span>{c.mood_emoji}</span>}{c.currently && <span className="currently"><strong>{c.currently.label}</strong> {c.currently.text}</span>}{c.status_text && <span>{c.status_text}</span>}</p>}
          {c.bio && <p className="bio">{c.bio}</p>}
          {c.song && <p className="preview-song"><MusicIcon /> <strong>{c.song.title || "Profile song"}</strong>{c.song.artist && <> · {c.song.artist}</>}{c.song_why && <em> — {c.song_why}</em>}</p>}
        </div>
      </div>
      {!!c.showcase?.length && <ul className="showcase-strip">{c.showcase.map(a => <li key={a.key} className="showcase-tile"><strong>{a.name}</strong></li>)}</ul>}
    </div>
    <a className="button small quiet" href={`/${c.username}`}>Open your page</a>
  </aside>;
}
