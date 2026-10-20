import { redirect } from "next/navigation";
import { Advertise } from "../../components/Advertise";
import { currentAccount } from "../session";
import "../../styles/profiles.css";

export const metadata = { title: "Advertise | S.V.E.R", robots: { index: false, follow: false } };

/** S.V.E.R's own ad network (docs/ADVERTISERS.md): approved ads, guaranteed verified impressions. */
export default async function AdvertisePage({ searchParams }: { searchParams: Promise<{ campaign?: string }> }) {
  if (!(await currentAccount())) redirect("/login");
  const paid = (await searchParams).campaign ?? null;
  return <div className="settings-page single"><div className="settings-body">
    <h1>Advertise</h1>
    <Advertise paid={paid} />
  </div></div>;
}
