import { ChannelFrame } from "../../../components/ChannelFrame";
import { channelMetadata, loadChannel, type ChannelParams } from "../../../lib/channel";

// Stub (docs/PROFILES.md, P8 and "Later-module stubs"): a reserved channel sub-path with no tab
// until the Valor economy exists (Module 4 / Phase 2).
export async function generateMetadata({ params }: { params: ChannelParams }) {
  return channelMetadata((await params).username, "Rewards");
}
export default async function RewardsStub({ params }: { params: ChannelParams }) {
  const data = await loadChannel((await params).username);
  const name = data.channel.username;
  return <ChannelFrame data={data} path={`/${name}/rewards`}>
    <section className="panel section rewards-stub" data-stub="rewards">
      <span className="badge">Coming soon</span>
      <h2>Rewards are coming</h2>
      <p className="muted">Channel rewards arrive with the Valor economy.</p>
    </section>
  </ChannelFrame>;
}
