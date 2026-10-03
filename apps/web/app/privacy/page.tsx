import type { Metadata } from "next";
import Link from "next/link";
import { PolicyPage } from "../../components/SitePage";

export const metadata: Metadata = {
  title: "Privacy Policy | S.V.E.R",
  description: "What information S.V.E.R collects, how it is used, and how to manage your data.",
  alternates: { canonical: "https://sver.tv/privacy" },
};

export default function PrivacyPage() {
  return <PolicyPage path="/privacy" title="Privacy Policy" intro="What we collect, why we need it, and the choices you have." summary={[
    "We use account, activity and technical information to operate and secure S.V.E.R.",
    "Your published channel details, follows and public posts can be seen by others.",
    "We do not sell personal information; service providers help us run the Platform.",
    "Manage your information in settings or contact us about a privacy request.",
  ]} sections={[
    { title: "Information we collect", content: <>
      <p>S.V.E.R collects information you provide when you create an account, use the Platform or contact us:</p>
      <ul>
        <li><strong>Account information:</strong> email, username, date of birth, password hash, verification status, linked sign-in accounts and account security settings. Passwords are stored as hashes, not readable passwords.</li>
        <li><strong>Channel information:</strong> display name, bio, images, links, follows, Wall posts, fan art, schedule and other details you choose to publish.</li>
        <li><strong>Streaming and chat:</strong> stream titles and categories, broadcast sessions, playback activity needed to count viewers, and chat messages.</li>
        <li><strong>Safety and support:</strong> reports, reported-content snapshots, moderation actions, appeals and messages you send us.</li>
        <li><strong>Technical information:</strong> IP addresses, browser and device information, session records and service logs used for operation, security and troubleshooting.</li>
      </ul>
      <p>When you sign in with Google, Twitch or Discord, we receive a provider account identifier and the email and profile information returned for the requested sign-in permissions. We use this to identify your account and complete sign-in or linking. We do not receive your password for that provider.</p>
      <p>Information transferred from an earlier S.V.E.R account may be retained to preserve that account and its profile, linked accounts and relationships.</p>
    </> },
    { title: "How we use information", content: <>
      <ul>
        <li>Create and secure accounts, verify age eligibility, authenticate sign-ins and recover access.</li>
        <li>Provide channel pages, following, streaming, chat and other features you use.</li>
        <li>Send verification, recovery, security and requested report-update emails.</li>
        <li>Prevent spam, abuse and unauthorized access; investigate reports and enforce our <Link href="/guidelines">Community Guidelines</Link>.</li>
        <li>Diagnose problems, maintain the service and respond to legal obligations and requests.</li>
      </ul>
    </> },
    { title: "Public information and sharing", content: <>
      <p>Your channel, username, display name, published profile details, public posts, and follower and following lists are visible to other people. Public chat can be read without an account. Do not publish information you want to keep private. Your email, date of birth and account security details are not part of your public profile.</p>
      <p>We do not sell your personal information or use third-party advertising cookies. We share information with service providers as needed to run S.V.E.R, and may disclose information when legally required or necessary to protect people, enforce our terms or address abuse.</p>
      <p>Providers include OVH for hosting, Cloudflare for network protection and Turnstile bot checks, Resend for email, and Google, Twitch and Discord when you choose their sign-in options. Media delivery may use object storage and Bunny CDN as those features become available. Fonts are served directly by S.V.E.R. These providers process connection information and other data needed to deliver their services under their own privacy policies.</p>
      <p>If you open external links or choose to load an embedded profile song, that third-party service receives your connection information and handles it under its own policy.</p>
    </> },
    { title: "Cookies and browser storage", content: <>
      <p>We use cookies to maintain sign-in sessions and protect authentication flows, including provider sign-in and two-factor checks. Session cookies last up to 30 days and renew with activity. You can revoke sessions in <Link href="/account">Account security</Link>.</p>
      <p>Browser storage may remember playback or song preferences and an anonymous playback identifier used to count active viewers. Blocking cookies or storage can prevent sign-in or affect those features. Turnstile and embedded services may use their own storage for security or their requested functionality.</p>
    </> },
    { title: "Retention and deletion", content: <>
      <p>We retain account information while your account is active. Account deletion has a 14-day grace period; you can cancel it by signing in during that time. After the grace period, the account erasure process removes account data and handles associated content.</p>
      <p>Ordinary chat messages expire after seven days. Reports may preserve a separate copy of reported text or media for review; report snapshots are scheduled for removal 90 days after resolution. Some moderation audit records remain to document actions taken.</p>
      <p>Deletion from the active service does not immediately erase backup copies. Backups expire through their retention cycle. Information needed for legal obligations, security investigations or preservation of an earlier account may require separate review. Contact us if you need help with such a record.</p>
    </> },
    { title: "Your choices and requests", content: <>
      <p>You can update your public information in <Link href="/settings/profile">Profile settings</Link>. In <Link href="/account">Account security</Link>, you can manage connected providers, revoke sessions and request deletion. You must keep at least one usable sign-in method when unlinking a provider.</p>
      <p>For a copy of your personal information, corrections, deletion or other privacy requests, email <a href="mailto:privacy@sver.tv">privacy@sver.tv</a>. You do not need to sign in to contact us. We may ask for information needed to verify ownership before disclosing or changing private data. Your rights and any exceptions depend on the law that applies to you.</p>
    </> },
    { title: "Security and children", content: <>
      <p>We use HTTPS, password hashing, session controls and two-factor authentication to protect accounts. No service can guarantee complete security. Keep credentials private and contact <a href="mailto:support@sver.tv">support@sver.tv</a> if you suspect account misuse.</p>
      <p>S.V.E.R is not intended for children under 13. We do not knowingly collect account information from children under 13. If you believe a child has provided personal information, email <a href="mailto:privacy@sver.tv">privacy@sver.tv</a>.</p>
    </> },
    { title: "International use, changes and contact", content: <>
      <p>S.V.E.R operates from the United States and uses service providers that may process data in other countries. Data protection laws may differ from those where you live.</p>
      <p>We will post updates here and notify you of significant changes on the Platform or by email. For questions, contact <a href="mailto:privacy@sver.tv">privacy@sver.tv</a> or visit <Link href="/contact">Contact</Link>.</p>
    </> },
  ]} />;
}
