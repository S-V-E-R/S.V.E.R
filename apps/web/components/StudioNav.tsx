import Link from "next/link";
import { SideMenu, type MenuGroup } from "./SideMenu";

// Go live comes first because going live is what most visits to the Studio are for. Each group stays short
// enough to scan (navigation review, October 2026): one long flat list was the wall people got lost in.
const GROUPS: MenuGroup[] = [
  ["Go live", [["/studio/stream", "Live stream"], ["/studio/magnet", "MAGNet"], ["/studio/restream", "Restream"], ["/studio/overlays", "Alerts & overlays"]]],
  ["Chat & community", [["/studio/chat", "Chat"], ["/studio/commands", "Commands & bot"], ["/studio/emotes", "Emotes"], ["/studio/linked-chat", "Linked chat"], ["/studio/discord", "Discord"], ["/studio/raids", "Raids & hosting"], ["/studio/guilds", "Guilds & co-streams"]]],
  ["Tools", [["/studio/rewards", "Rewards"], ["/studio/board", "Board"], ["/studio/counters", "Counters"]]],
  ["Channel page", [["/studio/channel", "Overview & War Council"], ["/settings/profile", "Your page"], ["/studio/channel/wall", "Wall"], ["/studio/channel/schedule", "Schedule"], ["/studio/channel/sponsors", "Sponsors"], ["/studio/channel/setup", "Streaming setup"], ["/studio/channel/blocks", "About me"], ["/studio/channel/fan-art", "Fan Art"]]],
  ["Earnings", [["/studio/payouts", "Payouts"], ["/studio/shine", "Shine & Moments"]]],
  ["Recordings", [["/studio/videos", "Videos & clips"], ["/studio/beacons", "Beacons"], ["/studio/copyright", "Copyright"]]],
];

/** Creator Studio navigation, grouped so the list scans quickly; the current page is marked. */
export function StudioNav({ username }: { username: string }) {
  return <SideMenu label="Creator Studio" groups={GROUPS} footer={<Link href={`/${username}`} className="studio-view">View channel</Link>} />;
}
