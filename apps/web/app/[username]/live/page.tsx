import { redirect } from "next/navigation";
import type { ChannelParams } from "../../../lib/channel";

// Module 3 defines the live view; until then the proxy sends a 302 to the channel (this is the fallback).
export default async function Live({ params }: { params: ChannelParams }) {
  redirect(`/${(await params).username}`);
}
