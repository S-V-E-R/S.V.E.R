import { notFound } from "next/navigation";
import { VideoWatch } from "../../../components/VideoWatch";
import { apiGet } from "../../../lib/server-api";
export const metadata = { title: "S.V.E.R clip", robots: { index: false, follow: false } };
// A deleted or unknown video is a real 404 (other refusals, such as age or visibility, render the player's own message).
export default async function Embed({ params }: { params: Promise<{ id: string }> }) { const { id } = await params; if ((await apiGet(`/api/videos/${encodeURIComponent(id)}`)).status === 404) notFound(); return <VideoWatch key={id} id={id} embed />; }
