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
    "Paid features run through Stripe. Purchases are non-refundable except as these terms or the law say.",
    "Creators earn by tier and are paid every two weeks; earnings are never guaranteed.",
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
      <p>You can rename once every 60 days. Case-only changes are allowed at any time. An old name is held for 30 days and redirects to your channel during that period. Username trading and paid claims are not offered. You can ask for a name held by an account unused for 12 months; its holder is emailed and keeps it by signing in within 30 days, otherwise staff may release it and the holder gets a generated name.</p>
    </> },
    { title: "Your content and our intellectual property", content: <>
      <p>You retain ownership of the content you create. By sharing it on S.V.E.R, you grant us a worldwide, non-exclusive, royalty-free license to host, display and distribute it on the Platform, enable interaction with it, and promote it on the Platform and in marketing materials. You must have the rights needed to grant this license.</p>
      <p>The S.V.E.R name, logo and faction emblems belong to S.V.E.R. Trademark use requires written permission except where the law permits it. The Platform source code is available under its published open-source license; these Terms do not replace that license.</p>
      <p>See <Link href="/dmca">Copyright & DMCA</Link> for infringement notices and counter-notices.</p>
    </> },
    { title: "Purchases", content: <>
      <p>Prices are in US dollars and are shown before you pay. Card payments are processed by Stripe; S.V.E.R never receives your card details. You need a verified email to buy anything.</p>
      <ul>
        <li><strong>Channel subscriptions</strong> cost $4.99, $9.99 or $24.99 a month. A card subscription renews every month until you cancel it. You can cancel anytime, and its benefits last until the end of the month you paid for. Upgrading takes effect at once, and the difference is charged prorated. You can instead pay for one month at a time with 499, 999 or 2,499 Purchased Valor, which doesn&apos;t renew.</li>
        <li><strong>Gift subscriptions</strong> give one month to a named viewer, or 5, 10 or 20 one-month gifts to random signed-in viewers who recently chatted in that channel and allow gifts. A gifted month never renews.</li>
        <li><strong>Purchased Valor</strong> is bought in packs shown in your <Link href="/wallet">wallet</Link>. You can spend it on tributes in chat (at least 10 Valor with a message), Skills and subscriptions. It doesn&apos;t expire, can&apos;t be transferred to another account and can&apos;t be cashed out. A tribute or Skill must follow the channel&apos;s chat rules, and nothing is spent if the message is refused.</li>
        <li><strong>Engagement Valor</strong> is earned by watching, chatting and following in a channel and can be spent only in that channel. It can&apos;t be bought, transferred or cashed out, and has no cash value.</li>
        <li><strong>Platform supporter subscriptions</strong> support S.V.E.R itself: Wolfsworn costs $5 a month and Greycloak $25 a month, by card only. They can&apos;t be paid with Valor or gifted. All of the money goes to S.V.E.R. The perks are cosmetic only: badges, a silver name and player-card frame, and &ldquo;Supporting since&rdquo; on your profile. Supporting never changes discovery, faction influence, XP, Valor or chat rules. Upgrading to Greycloak takes effect at once, prorated; downgrading takes effect at your next renewal; cancelling keeps your perks until the end of the paid month.</li>
        <li><strong>Founder marks:</strong> the first 500 accounts to pay for a full Greycloak month get a numbered Founder mark (#001 to #500), kept after cancelling or downgrading. An upgrade from Wolfsworn claims a mark at its first full Greycloak renewal. A mark can&apos;t be bought any other way, sold or transferred, and it is revoked if the payment that claimed it is fully refunded or lost to a dispute. See <Link href="/supporters">Support S.V.E.R</Link>.</li>
        <li><strong>Wall seats</strong> are not on sale. Their terms will be published here before sales open.</li>
      </ul>
      <p>If you are 13 to 17, or we don&apos;t have your date of birth, a parent or legal guardian must confirm at checkout that they are the cardholder and consent before your first purchase. Purchases by an account under 18 are limited to $50 per calendar month (UTC), including subscription renewals due that month.</p>
    </> },
    { title: "Refunds and chargebacks", content: <>
      <p>Purchases are non-refundable except as these Terms describe or where the law requires a refund. A parent or guardian can report a purchase made by a minor without their permission through <Link href="/contact">support</Link> for a refund.</p>
      <p>When a payment is refunded or disputed:</p>
      <ul>
        <li>The Purchased Valor it bought is removed in proportion to the amount refunded. If that Valor was already spent, your balance can go below zero, and spending is locked until it is back above zero. Valor from a dispute you win is restored.</li>
        <li>A full refund of the latest payment for a channel or platform supporter subscription ends its benefits at once and stops it from renewing.</li>
        <li>The creator&apos;s share of the payment is reversed, as described in Creator earnings and payouts.</li>
      </ul>
    </> },
    { title: "Creator earnings and payouts", content: <>
      <p><strong>Who can earn.</strong> Your channel can receive subscriptions, gift subs, tributes and Skills once you have a verified email, authenticator two-factor authentication and a finished Stripe Connect payout setup, including Stripe&apos;s tax form. There is no follower or hours minimum. If you are 13 to 17, a parent or legal guardian must own the payout account: they complete Stripe&apos;s setup, accept Stripe&apos;s Connected Account Agreement and receive the payouts. Stripe collects identity, bank and tax details; S.V.E.R never sees them. A suspended or restricted channel can&apos;t receive new payments; earnings already in your balance stay owed to you under these Terms.</p>
      <p><strong>What you earn.</strong> You keep 65%, 70%, 75% or 80% of the full price of each subscription and gift sub, depending on your <Link href="/help/creator-tiers">creator tier</Link>; S.V.E.R pays the card fees out of its share. You earn 0.8¢ for every Valor spent on you, at every tier. In a merged co-stream, money spent through the squad page is split equally among the members who are live and can earn at that moment, each at their own tier. Earnings build up in tenths of a cent and are rounded down only when paid.</p>
      <p><strong>When you&apos;re paid.</strong> Payday is every two weeks, on Fridays at noon Eastern: your whole available balance is sent by Stripe transfer and usually arrives in about two business days. Between paydays, Early Pay lets you withdraw up to 75% of what you&apos;ve earned since the last payday, less any earlier Early Pay, once per day (Eastern time). Standard Early Pay is free and takes about two business days. Instant Early Pay arrives in minutes to an eligible debit card, and Stripe&apos;s 1% fee is taken from the amount and shown before you confirm.</p>
      <p><strong>Refunds and disputes.</strong> If a payment you earned from is refunded or lost to a dispute, your share of it is reversed. If that share was already paid out, your balance goes below zero and is recovered from your future earnings before your next payout or Early Pay.</p>
      <p><strong>Taxes.</strong> S.V.E.R doesn&apos;t withhold taxes from your earnings. Stripe collects your tax information and handles your tax forms, and you are responsible for the taxes you owe.</p>
      <p><strong>No guarantee.</strong> These Terms don&apos;t promise any amount of earnings, any creator tier or any number of viewers or supporters.</p>
    </> },
    { title: "Feature availability", content: <>
      <p>Features may change or be unavailable as S.V.E.R develops. Ads, ad revenue sharing and Ad Valor are not available yet: no ad money is paid to creators or viewers, and Ad Valor can&apos;t be earned, spent or cashed out. Terms for these and any other new paid features will be published before they become available.</p>
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
      <p>Intimate images shared without consent are prohibited, including sexual deepfakes of real people. The person shown or their authorized representative can submit a <Link href="/take-it-down">Take It Down request</Link> without an account. Valid requests lead to permanent content removal, a level-three strike and account-ban review. Knowingly false removal requests violate these Terms. An account sanction may be appealed; a valid removal is not reversed.</p>
      <p>We may update these Terms. We will give notice of material changes on the Platform or by email. Continued use after changes take effect constitutes acceptance to the extent permitted by law. If a provision is unenforceable, it will be modified only as needed, and the remaining provisions continue to apply.</p>
      <p>Questions about these Terms: <a href="mailto:legal@sver.tv">legal@sver.tv</a>. Account help: <Link href="/contact">Contact S.V.E.R</Link>.</p>
    </> },
  ]} />;
}
