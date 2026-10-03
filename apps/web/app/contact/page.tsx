import type { Metadata } from "next";
import Link from "next/link";
import SitePage from "../../components/SitePage";

export const metadata: Metadata = {
  title: "Contact | S.V.E.R",
  description: "Contact S.V.E.R for account help, safety reports, privacy requests, copyright and appeals.",
  alternates: { canonical: "https://sver.tv/contact" },
};

export default function ContactPage() {
  return <SitePage path="/contact" title="Contact S.V.E.R" intro="Choose the right contact for your question. You do not need an account to email us.">
    <section>
      <h2>Account and technical help</h2>
      <p><a href="mailto:support@sver.tv">support@sver.tv</a></p>
      <p>Include your username, what went wrong and any error message. Never send passwords, authenticator or recovery codes, or stream keys. For a forgotten password, start with <Link href="/forgot">account recovery</Link>.</p>
    </section>
    <section>
      <h2>Safety and moderation</h2>
      <p><a href="mailto:safety@sver.tv">safety@sver.tv</a></p>
      <p>Report harassment, threats, impersonation or harmful content. Include the S.V.E.R page URL and a description; do not attach harmful images. Review the <Link href="/guidelines">Community Guidelines</Link> for our content rules. If someone faces immediate danger, contact local emergency services.</p>
    </section>
    <section>
      <h2>Privacy</h2>
      <p><a href="mailto:privacy@sver.tv">privacy@sver.tv</a></p>
      <p>Request access, correction or deletion of personal information, or ask about the <Link href="/privacy">Privacy Policy</Link>. Profile editing and account deletion are also available in your account settings.</p>
    </section>
    <section>
      <h2>Copyright and legal</h2>
      <p>Copyright notices and counter-notices: <a href="mailto:dmca@sver.tv">dmca@sver.tv</a>. Please read <Link href="/dmca">Copyright & DMCA</Link> first.</p>
      <p>Other legal inquiries and formal correspondence: <a href="mailto:legal@sver.tv">legal@sver.tv</a>.</p>
    </section>
    <section>
      <h2>Appeals</h2>
      <p><a href="mailto:appeals@sver.tv">appeals@sver.tv</a></p>
      <p>Include your username and the decision or case reference. If you can sign in, review platform actions and available appeals in <Link href="/settings/standing">Account standing</Link>.</p>
    </section>
  </SitePage>;
}
