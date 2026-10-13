import Queue from "./queue";
export default async function Page({ searchParams }: { searchParams: Promise<{ reason?: string }> }) {
  const { reason } = await searchParams;
  return <Queue reason={reason === "removal_match" ? reason : ""} />;
}
