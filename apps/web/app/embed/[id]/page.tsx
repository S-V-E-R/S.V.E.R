import { VideoWatch } from "../../../components/VideoWatch";
export const metadata = { title: "S.V.E.R clip" };
export default async function Embed({ params }: { params: Promise<{ id: string }> }) { const { id } = await params; return <VideoWatch key={id} id={id} embed />; }
