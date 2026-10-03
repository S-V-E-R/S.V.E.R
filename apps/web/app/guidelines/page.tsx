import type { Metadata } from "next";
import Link from "next/link";
import { PolicyPage } from "../../components/SitePage";

export const metadata: Metadata = {
  title: "Community Guidelines | S.V.E.R",
  description: "What you can stream and share on S.V.E.R, community conduct, reports and appeals.",
  alternates: { canonical: "https://sver.tv/guidelines" },
};

export default function GuidelinesPage() {
  return <PolicyPage path="/guidelines" title="Community Guidelines" intro="Make something. Share the moment. Respect the people around you." summary={[
    "Play, build or make: no reaction streams, gambling or just chatting streams.",
    "Do not share abusive, illegal or sexually explicit content or violate consent.",
    "Respect copyright, other people and the rules of each channel.",
    "Report problems using the site controls or our safety email; appeals are available.",
  ]} sections={[
    { title: "Play, build or make", content: <>
      <p>S.V.E.R welcomes gaming, art, crafting and making, music, and education. Your stream should center on something you are playing, building or making. Talking with your community while you do that is welcome.</p>
      <p><strong>No reaction streams, gambling or just chatting streams.</strong> Choose a category and title that accurately describe what you are doing. Creators are welcome to multistream.</p>
    </> },
    { title: "Keep people safe", content: <>
      <p>These rules cover streams, profile images and text, Wall posts, fan art, links, usernames and chat. Do not share or encourage:</p>
      <ul>
        <li>Illegal content, child exploitation or sexual content involving minors.</li>
        <li>Pornography, sexually explicit material or intimate images shared without consent, including fabricated or AI-generated images.</li>
        <li>Harassment, stalking, threats, bullying, or revealing someone else’s private information without permission.</li>
        <li>Hate or violence directed at people because of protected characteristics.</li>
        <li>Graphic real-world violence or gore, promotion of self-harm, or dangerous acts that put people at risk.</li>
        <li>Scams, phishing, malware, spam or deceptive impersonation.</li>
      </ul>
      <p>Content warnings do not permit otherwise prohibited content. Faction rivalry must stay friendly and must never become targeted harassment.</p>
    </> },
    { title: "Respect creators and the community", content: <>
      <p>Share only content you own or are authorized to use. This includes music, video, images and profile songs. See <Link href="/dmca">Copyright & DMCA</Link> for reporting infringement.</p>
      <p>Do not inflate viewer counts or other activity with bots, coordinate abuse, evade bans or mislead people about who you are. Follow the rules of the channel you visit. Streamers should moderate their communities, and moderators must use their tools responsibly.</p>
    </> },
    { title: "Report a problem", content: <>
      <p>Use the report controls on a channel, Wall post or fan-art item, or email <a href="mailto:safety@sver.tv">safety@sver.tv</a>. You can contact us without an account. Include the page URL and enough detail to locate the issue. Do not resend harmful images or include passwords, authenticator codes or stream keys.</p>
      <p>You can follow reports submitted through the site in <Link href="/settings/reports">My reports</Link>. If someone faces immediate danger, contact local emergency services.</p>
    </> },
    { title: "Enforcement and appeals", content: <>
      <p>Depending on severity and repetition, violations can lead to content removal, chat restrictions, temporary suspension or permanent account termination. Unlawful conduct may be referred to law enforcement.</p>
      <p>Platform actions and available appeals appear in <Link href="/settings/standing">Account standing</Link>. If you cannot access your account, email <a href="mailto:appeals@sver.tv">appeals@sver.tv</a> with your username and the decision you want reviewed. Channel owners and moderators also enforce their own channel rules.</p>
    </> },
  ]} />;
}
