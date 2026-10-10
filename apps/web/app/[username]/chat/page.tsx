import { Chat } from "../../../components/Chat";
import { channelMetadata, loadChannel, type ChannelParams } from "../../../lib/channel";
import { apiGet } from "../../../lib/server-api";
import { currentAccount } from "../../session";
import "../../../styles/profiles.css";

export async function generateMetadata({ params }: { params: ChannelParams }) {
  return channelMetadata((await params).username, "Chat");
}

/**
 * /{username}/chat: the channel's chat alone, for a second window (Pop out), an OBS dock (?dock=1)
 * or a read-only OBS browser source (?overlay=1). docs/CHANNEL_ADDITIONS.md "Pop-out chat".
 */
export default async function PopoutChat({ params, searchParams }: { params: ChannelParams; searchParams: Promise<{ dock?: string; overlay?: string }> }) {
  const c = (await loadChannel((await params).username)).channel;
  const query = await searchParams;
  const variant = query.overlay === "1" ? "overlay" : query.dock === "1" ? "dock" : "popout";
  const [account, chat] = await Promise.all([
    currentAccount(),
    variant === "overlay" ? apiGet<{ overlay_fade_seconds: number }>(`/api/channels/${encodeURIComponent(c.username)}/chat`) : null,
  ]);
  return <div className={`popout-chat popout-${variant}`} data-theme={c.faction ?? "neutral"}>
    {/* The overlay is a transparent OBS browser source. */}
    {variant === "overlay" && <style>{"html,body{background:transparent!important}"}</style>}
    <Chat username={c.username} account={variant === "overlay" ? null : account?.username ?? null} variant={variant} fade={chat?.data?.overlay_fade_seconds ?? 30} />
  </div>;
}
