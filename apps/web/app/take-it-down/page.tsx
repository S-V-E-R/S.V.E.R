import type { Metadata } from "next";
import Link from "next/link";
import { PolicyPage } from "../../components/SitePage";
import { apiGet } from "../../lib/server-api";
import RequestForms from "./request-forms";

export const metadata: Metadata = {
  title: "Take It Down requests | S.V.E.R",
  description: "Request removal of an intimate image shared without consent. No account is required.",
  alternates: { canonical: "https://sver.tv/take-it-down" },
  referrer: "no-referrer",
};

export default async function TakeItDownPage({ searchParams }: { searchParams: Promise<{ location?: string }> }) {
  const { location } = await searchParams;
  const config = await apiGet<{ turnstile_site_key: string }>("/api/auth/config");
  return <PolicyPage path="/take-it-down" title="Take It Down requests" intro="Request removal of an intimate image or video shared without consent. You do not need a S.V.E.R account." summary={[
    "You can report an intimate image of yourself, or act with authorization for the person shown.",
    "This includes realistic images made or altered with software or AI.",
    "Give us links and location details. Do not upload or send a copy of the image.",
    "Valid requests are handled as soon as possible, within 48 hours, including weekends and holidays.",
  ]} sections={[
    { title: "Request removal or check a request", content: config.data?.turnstile_site_key
      ? <RequestForms sitekey={config.data.turnstile_site_key} location={typeof location === "string" ? location.slice(0,2048) : ""} />
      : <p role="alert">The request form could not load. Please try again, or email <a href="mailto:safety@sver.tv">safety@sver.tv</a> with the content’s location, your contact details, signature, authority to act and good-faith statement. Do not attach an image.</p> },
    { title: "What happens next", content: <>
      <p>You receive a request number on this page and by email. We hide identifiable stored images while reviewing your request. Staff stop reported live streams during review. We may contact you for more location details; the deadline continues to run.</p>
      <p>For a valid request, we remove the content and known identical copies and block matching uploads. If a request is mistaken or invalid, we explain why and restore content hidden by that request where no other removal applies. The uploader is not penalized for a mistaken request.</p>
      <p>The uploader is notified of a valid removal and may appeal an account sanction. A valid Take It Down removal is permanent. We do not share your contact details with the uploader.</p>
    </> },
    { title: "Images involving a minor", content: <p>Tell us in the location details if the person shown was under 18. Staff prioritize removal, preserve records required by law, and report suspected child sexual exploitation to NCMEC’s CyberTipline. Do not download, upload or email a copy to make a request.</p> },
    { title: "Your information", content: <>
      <p>We keep your request, contact details, signature, location information, review actions and notices for three years. Only authorized staff can access them, for handling the request and checking our response times. A legal preservation duty can require longer retention of relevant evidence.</p>
      <p>Knowingly false requests violate our <Link href="/terms">Terms</Link>. If S.V.E.R fails to handle a valid request, you can <a href="https://takeitdown.ftc.gov/">report the platform to the FTC</a>.</p>
    </> },
  ]} />;
}
