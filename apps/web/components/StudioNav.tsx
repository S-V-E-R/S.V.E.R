import Link from "next/link";
import { SideMenu, type MenuGroup } from "./SideMenu";

// Stream comes first because going live is what most visits to the Studio are for.
const GROUPS: MenuGroup[] = [
  ["Stream", [["/studio/stream", "Live stream"], ["/studio/chat", "Chat"], ["/studio/commands", "Commands & bot"], ["/studio/emotes", "Emotes"], ["/studio/raids", "Raids & hosting"], ["/studio/restream", "Restream"], ["/studio/linked-chat", "Linked chat"], ["/studio/discord", "Discord"], ["/studio/overlays", "Alerts & overlays"], ["/studio/magnet", "MAGNet"], ["/studio/rewards", "Rewards"], ["/studio/board", "Board"], ["/studio/counters", "Counters"]]],
  ["Channel page", [["/studio/channel", "Overview"], ["/settings/profile", "Your page"], ["/studio/channel/war-council", "War Council"], ["/studio/channel/wall", "Wall"], ["/studio/channel/schedule", "Schedule"], ["/studio/channel/sponsors", "Sponsors"], ["/studio/channel/setup", "Streaming setup"], ["/studio/channel/blocks", "About me"], ["/studio/channel/fan-art", "Fan Art"]]],
  ["Growth", [["/studio/growth", "Growth"]]],
  ["Earnings", [["/studio/payouts", "Payouts"], ["/studio/shine", "Shine"], ["/studio/moments", "Shine Moments"]]],
  ["Teams", [["/studio/guilds", "Guilds"], ["/studio/squads", "Co-streams"]]],
  ["Recordings", [["/studio/videos", "Videos & clips"], ["/studio/beacons", "Beacons"], ["/studio/copyright", "Copyright"]]],
];

/** Creator Studio navigation, grouped so the list scans quickly; the current page is marked. */
export function StudioNav({ username }: { username: string }) {
  return <SideMenu label="Creator Studio" groups={GROUPS} footer={<Link href={`/${username}`} className="studio-view">View channel</Link>} />;
}
