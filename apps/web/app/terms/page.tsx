import type { Metadata } from "next";
import Link from "next/link";
import { PolicyPage } from "../../components/SitePage";

export const metadata: Metadata = {
  title: "Terms of Service | S.V.E.R",
  description: "The terms for accounts, content and community participation on S.V.E.R.",
  alternates: { canonical: "https://sver.tv/terms" },
};

export default function TermsPage() {
  return <PolicyPage path="/terms" title="Terms of Service" intro="The rules for using S.V.E.R and sharing your work here." summary={[
    "You must be at least 13 and keep your account credentials private.",
    "Stream something you play, build or make; follow our content and conduct rules.",
    "You keep ownership of your content and grant the permissions described below.",
    "Read the full terms for moderation, account closure, liability and dispute rules.",
  ]} sections={[
    { title: "Using S.V.E.R", content: <>
      <p>These Terms govern your use of SVER.TV and its related services (the Platform). By creating an account or using the Platform, you agree to these Terms and our <Link href="/guidelines">Community Guidelines</Link>. Our <Link href="/privacy">Privacy Policy</Link> explains how we handle your information. If you do not agree to these Terms, do not use the Platform.</p>
    </> },
    { title: "Your account", content: <>
      <ul>
        <li>You must be at least 13 years old and provide accurate account information.</li>
        <li>Keep your password, authenticator codes, recovery codes and stream key private. Do not share your account.</li>
        <li>Maintain only one account per person. Using another account to evade a restriction or ban is prohibited.</li>
        <li>You are responsible for activity on your account. Contact <a href="mailto:support@sver.tv">support@sver.tv</a> if you suspect unauthorized access.</li>
      </ul>
      <p>Email verification is required to chat or stream. Streaming also requires two-factor authentication.</p>
    </> },
    { title: "Content and conduct", content: <>
      <p>S.V.E.R is for people who play, build or make: gaming, art, crafting and making, music, and education. Reaction streams, gambling and just chatting streams are not allowed. Creators are welcome to multistream.</p>
      <p>Do not publish illegal content, sexually explicit material, non-consensual intimate images, threats, harassment, hate speech, scams or private information about others. Only share material you have the right to use. Do not manipulate viewer counts, spam, impersonate others or evade moderation.</p>
      <p>The <Link href="/guidelines">Community Guidelines</Link> apply to streams, channel pages, images, links, chat and other participation. A content warning does not make prohibited content acceptable. Channel owners and moderators may also enforce their channel rules.</p>
    </> },
    { title: "Usernames", content: <>
      <p>Usernames are unique without regard to capitalization and use 3–25 letters, numbers or underscores. Reserved names, impersonation and abusive names are prohibited. Display names do not replace your unique username.</p>
      <p>You can rename once every 60 days. Case-only changes are allowed at any time. An old name is held for 30 days and redirects to your channel during that period. Username trading and paid claims are not offered.</p>
    </> },
    { title: "Your content and our intellectual property", content: <>
      <p>You retain ownership of the content you create. By sharing it on S.V.E.R, you grant us a worldwide, non-exclusive, royalty-free license to host, display and distribute it on the Platform, enable interaction with it, and promote it on the Platform and in marketing materials. You must have the rights needed to grant this license.</p>
      <p>The S.V.E.R name, logo and faction emblems belong to S.V.E.R. Trademark use requires written permission except where the law permits it. The Platform source code is available under its published open-source license; these Terms do not replace that license.</p>
      <p>See <Link href="/dmca">Copyright & DMCA</Link> for infringement notices and counter-notices.</p>
    </> },
    { title: "Feature availability", content: <>
      <p>Features may change or be unavailable as S.V.E.R develops. Subscriptions, tips, Valor purchases and payouts are not available in the rebuilt Platform. These Terms do not promise earnings or a payment entitlement. Terms for paid features will be published before those features become available.</p>
    </> },
    { title: "Moderation and account closure", content: <>
      <p>We may remove content, restrict access, suspend or terminate accounts for violations of these Terms or the Community Guidelines, unlawful conduct, fraud, metric manipulation or ban evasion. Channel moderators may delete chat messages, time out users and ban users from their channels.</p>
      <p>You can review platform actions and available appeals in <Link href="/settings/standing">Account standing</Link>, or contact <a href="mailto:appeals@sver.tv">appeals@sver.tv</a>. Include your username and details of the decision.</p>
      <p>You can request account deletion in <Link href="/account">Account security</Link>. There is a 14-day grace period during which you can cancel deletion. The Privacy Policy describes retention of reports, safety records and backups.</p>
    </> },
    { title: "Disclaimers", content: <>
      <p>To the extent permitted by law, the Platform is provided “as is” and “as available,” without express or implied warranties. We do not guarantee uninterrupted, error-free or secure service. We are not responsible for user-generated content, actions of other users, third-party services, or loss of data or revenue from technical issues, except where the law requires otherwise.</p>
    </> },
    { title: "Limitation of liability", content: <>
      <p>To the maximum extent permitted by law, S.V.E.R and its officers, directors, employees and agents are not liable for indirect, incidental, special, consequential or punitive damages, including lost profits, data or goodwill, arising from your use of the Platform.</p>
      <p>Our total liability for claims arising from your use of the Platform will not exceed the greater of the amount you paid us in the 12 months preceding the claim or $100 USD. These limitations do not exclude rights or liabilities that cannot lawfully be excluded.</p>
    </> },
    { title: "Indemnification", content: <>
      <p>To the extent permitted by law, you agree to indemnify, defend and hold harmless S.V.E.R and its affiliates from claims, damages, losses and expenses, including legal fees, arising from your use of the Platform, your content, or your violation of these Terms or third-party rights.</p>
    </> },
    { title: "Disputes and governing law", content: <>
      <p>To the extent permitted by law, disputes arising from these Terms or your use of the Platform will be resolved through binding arbitration under the rules of the American Arbitration Association, and you waive participation in class actions against S.V.E.R. Either party may seek injunctive relief in a court of competent jurisdiction for intellectual property violations.</p>
      <p>These Terms are governed by United States and Delaware law, without regard to conflict-of-law principles, subject to any mandatory protections that apply where you live.</p>
    </> },
    { title: "Changes and contact", content: <>
      <p>We may update these Terms. We will give notice of material changes on the Platform or by email. Continued use after changes take effect constitutes acceptance to the extent permitted by law. If a provision is unenforceable, it will be modified only as needed, and the remaining provisions continue to apply.</p>
      <p>Questions about these Terms: <a href="mailto:legal@sver.tv">legal@sver.tv</a>. Account help: <Link href="/contact">Contact S.V.E.R</Link>.</p>
    </> },
  ]} />;
}
