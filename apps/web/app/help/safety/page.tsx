import type { Metadata } from "next";
import Link from "next/link";
import SitePage from "../../../components/SitePage";

export const metadata: Metadata = {
  title: "Safety and fair discovery | S.V.E.R",
  description: "The chat moderation tools every S.V.E.R channel has, how shield mode and hate-raid defenses work, how viewbots are kept out of counts, and how MAGNet keeps discovery fair.",
  alternates: { canonical: "https://sver.tv/help/safety" },
};

/** Public summary of channel moderation, viewer integrity and MAGNet fairness (docs/LIVE_STREAMS.md, docs/MAGNET.md). */
export default function SafetyPage() {
  return <SitePage path="/help/safety" title="Safety and fair discovery" intro="The tools every channel gets to keep chat safe, how viewbots are kept out of the numbers, and why MAGNet can't be gamed.">
    <section>
      <h2>Chat moderation</h2>
      <p>Every channel has these from its first stream, set in <Link href="/studio/chat">Creator Studio → Chat</Link>:</p>
      <ul>
        <li><strong>Moderators.</strong> Appoint people you trust. They can delete messages, time out and ban chatters, and change chat settings. They can&apos;t act on you, each other or staff.</li>
        <li><strong>Timeouts and bans</strong> with a reason. A channel ban stops chat and signed-in watching.</li>
        <li><strong>Slow mode, link blocking and banned words</strong> (up to 200 words or phrases, matched regardless of case).</li>
        <li><strong>Minimum account age</strong> for chatters: 1, 7, 30 or 90 days.</li>
        <li><strong>Raids only from channels you follow</strong>, if you turn it on.</li>
        <li><strong>Blocks.</strong> Someone you block can&apos;t chat in your channel.</li>
      </ul>
    </section>
    <section>
      <h2>Shield mode and hate raids</h2>
      <ul>
        <li><strong>Shield mode</strong> puts several protections up at once with one click from chat, for 10, 30 or 60 minutes. While it&apos;s on, only followers of at least 10 minutes with accounts at least 7 days old can chat, links are blocked, slow mode is at least 10 seconds, and incoming raids are refused. It always ends by itself, and you can end it early.</li>
        <li><strong>Spike prompt.</strong> When a sudden wave of new viewers arrives that a raid, a MAGNet feature or your go-live alert doesn&apos;t explain, you and your moderators are offered shield mode in one click. It&apos;s never turned on for you.</li>
        <li><strong>Sweep.</strong> After a raid, ban in one action every new account (1 to 90 days old) that chatted or followed in the last few minutes. Their messages and follows are removed, and each ban can be reviewed or lifted.</li>
        <li><strong>Follow alerts</strong> only fire once a follow counts (15 minutes of live watching), and usernames containing your banned words never reach your overlay.</li>
      </ul>
    </section>
    <section>
      <h2>Viewbots don&apos;t count</h2>
      <ul>
        <li>A viewer counts only after their player has really been playing for a minute and passes the checks. Paid view services and bot farms show up as sessions that never count.</li>
        <li>Creator tiers, payouts and faction influence go further: they only count signed-in, verified accounts that watched for a couple of minutes with the page visible.</li>
        <li>No single signal decides. Several signals have to agree, so a shared home, school or mobile network is fine.</li>
        <li><strong>Bots sent at your channel never punish you.</strong> They just don&apos;t count. Only staff, with evidence that you or someone acting for you caused the traffic, can hold payouts, pause tier promotion or issue a strike, and you can appeal.</li>
        <li>After a stream, Creator Studio shows how many viewers weren&apos;t counted. Nobody, not even you, sees who or why, and raw IP addresses are never stored.</li>
      </ul>
    </section>
    <section id="magnet">
      <h2>How MAGNet stays fair</h2>
      <p><Link href="/magnet">MAGNet</Link> gives every live stream a turn at the top of Live Now and on MAGNet channels, whether it has 3 viewers or 3,000.</p>
      <ul>
        <li><strong>Never by size.</strong> No list or score uses viewer count or follower count. A moment (a chat burst, a follow burst, a raid, a win) is measured against each stream&apos;s own normal level.</li>
        <li><strong>No money.</strong> Subscriptions, tributes and paid board presses never raise a stream&apos;s chances.</li>
        <li><strong>Only real people.</strong> Moments count only verified accounts and viewers who passed the checks above. Messages arriving from MAGNet itself don&apos;t count, so being featured can&apos;t keep a stream featured.</li>
        <li><strong>Every turn comes.</strong> Moment switches alternate with fair turns for the stream that has waited longest, so every eligible stream is featured within a bounded time.</li>
        <li><strong>Who can be featured:</strong> streams live for at least a minute in an allowed category, from channels that aren&apos;t restricted and have no open viewbot case. Any channel can opt out in Creator Studio.</li>
      </ul>
      <p>Whether discovery works is public: <Link href="/open-data">Open data</Link> publishes homepage click-through, how many streams got a turn and how much watching came from discovery. Your own turns, features and the viewers they brought are in <Link href="/studio/analytics">Creator Studio → Analytics</Link>.</p>
    </section>
    <section>
      <h2>Reports and appeals</h2>
      <p>Report a channel, stream, chat message or post from its menu, or email <a href="mailto:safety@sver.tv">safety@sver.tv</a>. Staff review every report; no number of reports acts on its own. How rules are enforced and appealed is in the <Link href="/guidelines">Community Guidelines</Link>, and your own standing is in <Link href="/settings/standing">Account standing</Link>.</p>
    </section>
  </SitePage>;
}
