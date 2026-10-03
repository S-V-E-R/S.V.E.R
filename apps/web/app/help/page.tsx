import type { Metadata } from "next";
import Link from "next/link";
import SitePage from "../../components/SitePage";

export const metadata: Metadata = {
  title: "Help & FAQ | S.V.E.R",
  description: "Help with S.V.E.R accounts, profiles, watching, streaming setup, safety and support.",
  alternates: { canonical: "https://sver.tv/help" },
};

export default function HelpPage() {
  return <SitePage path="/help" title="Help & FAQ" intro="Find your way around accounts, channels and streaming.">
    <p className="notice">Accounts, channel profiles and profile uploads are available. Live delivery is still being prepared. Creator Studio shows whether streaming is available for your account.</p>
    <nav className="site-links" aria-label="Help topics">
      <a href="#accounts">Accounts</a><a href="#channels">Channels & watching</a><a href="#streaming">Streaming</a><a href="#safety">Safety & support</a>
    </nav>
    <section id="accounts">
      <h2>Accounts</h2>
      <details><summary>How do I create an account?</summary><p>Use <Link href="/signup">Create account</Link> with email and a password, or continue with Google, Twitch or Discord when available. You must be at least 13. New provider signups finish by confirming a username and date of birth. Verify your email before chatting or streaming.</p></details>
      <details><summary>Can I use my existing S.V.E.R account?</summary><p>Existing accounts and profiles have been carried into the rebuild. Use your existing email and password or linked Google, Twitch or Discord account. If you cannot sign in, try <Link href="/forgot">password recovery</Link> or <Link href="/contact">contact support</Link>. Contact support if something from your profile is missing.</p></details>
      <details><summary>Where are security settings and two-factor authentication?</summary><p>Open <Link href="/account">Account security</Link> to set up an authenticator, save recovery codes, manage linked providers and revoke sessions. Keep at least one sign-in method. Two-factor authentication is optional for viewers and required to access a stream key.</p></details>
      <details><summary>What if I forget my password or lose my authenticator?</summary><p>Use <Link href="/forgot">password recovery</Link> for a reset email. If two-factor authentication is enabled, use an unused recovery code when you cannot access your authenticator. A password reset does not disable two-factor authentication. If you have no recovery code, <Link href="/contact">contact support</Link>; never send your password or security codes.</p></details>
      <details><summary>How do I delete my account?</summary><p>Request deletion in <Link href="/account">Account security</Link>. You have 14 days to sign in and cancel before erasure. See the <Link href="/privacy">Privacy Policy</Link> for how reports, safety records and backups are handled.</p></details>
    </section>
    <section id="channels">
      <h2>Channels & watching</h2>
      <details><summary>Where is my channel, and how do I edit it?</summary><p>Your channel is at <code>sver.tv/your_username</code>. Use My channel when signed in. Edit your display name, bio, images and links in <Link href="/settings/profile">Profile settings</Link>. Channel branding, your Wall and schedule live in <Link href="/studio/channel">Creator Studio</Link>.</p></details>
      <details><summary>Can I change my username?</summary><p>Yes, in <Link href="/settings/profile">Profile settings</Link>, once every 60 days. Usernames use 3–25 letters, numbers or underscores. Capitalization-only changes are always allowed. Your previous name is held and redirects to your channel for 30 days.</p></details>
      <details><summary>Do I need an account to watch?</summary><p>Public channels can be viewed without signing in. When a creator is live and playback is available, their Live tab opens the stream. An offline channel shows its offline state. Sign in to follow a channel, and verify your email to chat. Your followed channels appear in <Link href="/following">Following</Link>.</p></details>
      <details><summary>Where are Browse, factions and viewer rewards?</summary><p>Browse and fair-rotation discovery arrive with MAGNet. The faction war, Valor, subscriptions and payouts are not available yet. Visit <Link href="/about">About S.V.E.R</Link> for the direction of the project and the current development roadmap.</p></details>
    </section>
    <section id="streaming">
      <h2>Streaming</h2>
      <details><summary>Who can stream, and do I need a special app?</summary><p>Streaming requires a verified email and two-factor authentication. No application or follower threshold is required. Use OBS or other software that supports RTMP. The S.V.E.R desktop app is optional, and multistreaming is welcome. Availability depends on the streaming setup shown in <Link href="/studio/stream">Creator Studio</Link>.</p></details>
      <details><summary>Where do I get my server address and stream key?</summary><p>Open <Link href="/studio/stream">Creator Studio → Live stream</Link>, set a title and category, and complete the security check to create or reveal a key. Copy the Server and Stream key fields into your broadcasting software. Always use those fields; do not reuse a server address or key from the old site.</p><p>If Studio says streaming is not configured, keys are unavailable until setup is finished. A key is private: replacing it disconnects the current publisher, and you must update OBS before reconnecting.</p></details>
      <details><summary>What OBS settings should I use?</summary><p>When streaming is available, choose a custom RTMP service and copy the server and key from Studio. Use these initial encoder settings:</p><ul><li>H.264 video and AAC audio.</li><li>A one-second keyframe interval.</li><li>B-frames off (zero).</li><li>Start testing at 6 Mbps video and 160 Kbps audio, up to 1080p at 60 frames per second.</li></ul><p>The bitrate guidance is provisional while delivery testing continues. Lower resolution or bitrate if your connection cannot sustain it. Studio shows the latest setup guidance and available input-health readings.</p></details>
      <details><summary>What happens if OBS disconnects?</summary><p>A broadcast has a 60-second reconnect window so a brief drop can continue the same session. Check your connection and the status in Studio. Replacing or revoking the key requires a new key in OBS; simply reconnecting with the old one will not work.</p></details>
      <details><summary>What can I stream?</summary><p>Gaming, art, crafting and making, music, and education. No reaction streams, gambling or just chatting streams. Talking to your viewers while you play, build or make is welcome. Read the <Link href="/guidelines">Community Guidelines</Link> before going live.</p></details>
    </section>
    <section id="safety">
      <h2>Safety & support</h2>
      <details><summary>How do I report content or appeal a decision?</summary><p>Use report controls on channels, Wall posts and fan art, or contact <a href="mailto:safety@sver.tv">safety@sver.tv</a> with the relevant URL. You do not need an account to email. Track site reports in <Link href="/settings/reports">My reports</Link> and review platform actions or available appeals in <Link href="/settings/standing">Account standing</Link>.</p></details>
      <details><summary>How do I get help with something else?</summary><p>Visit <Link href="/contact">Contact</Link> for account help, privacy requests, copyright notices and appeals. Include your username and a description of the issue. Never send passwords, authenticator codes, recovery codes or stream keys.</p></details>
    </section>
  </SitePage>;
}
