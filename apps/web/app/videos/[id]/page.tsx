import { notFound } from "next/navigation";
import { apiGet } from "../../../lib/server-api";
import type { VideoPage } from "../../../lib/videos";
import { VideoWatch } from "../../../components/VideoWatch";
export const metadata = { title: "Recording | S.V.E.R" };
export default async function Recording({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const result = await apiGet<VideoPage>(`/api/videos/${encodeURIComponent(id)}`);
  if (result.status === 404) notFound();
  return <VideoWatch key={id} id={id} initial={result.data} />;
}
