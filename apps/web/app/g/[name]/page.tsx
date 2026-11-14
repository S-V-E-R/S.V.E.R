import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { cache } from "react";
import { publicMeta } from "../../../lib/seo";
import { GuildView } from "../../../components/Guilds";
import type { GuildPage } from "../../../lib/guilds";
import { apiGet } from "../../../lib/server-api";
import { currentAccount } from "../../session";
import "../../../styles/profiles.css";
import "../../../styles/teams.css";
const loadGuild = cache(async (name: string) => apiGet<GuildPage>(`/api/guilds/${encodeURIComponent(name)}`));

export async function generateMetadata({ params }: { params: Promise<{ name: string }> }): Promise<Metadata> {
  const { data } = await loadGuild((await params).name);
  if (!data || data.guild.status !== "ACTIVE") return { title: "Guild | S.V.E.R", robots: { index: false, follow: false } };
  const description = data.guild.tagline || data.guild.about.slice(0, 160) || `${data.guild.name} on S.V.E.R`;
  return publicMeta(`${data.guild.name} | S.V.E.R`, description, `/g/${data.guild.slug}`, data.guild.avatar ?? undefined);
}

export default async function Guild({ params }: { params: Promise<{ name: string }> }) {
  const { name } = await params;
  const [result, account] = await Promise.all([loadGuild(name), currentAccount()]);
  if (result.status === 404) notFound();
  return result.data ? <GuildView initial={result.data} account={account?.username ?? null} /> : <p role="alert">This guild could not be loaded. Please try again.</p>;
}
