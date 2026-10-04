import { Unsubscribe } from "../../components/Notifications";
import "../../styles/profiles.css";

export const metadata = { title: "Unsubscribe | S.V.E.R", robots: { index: false, follow: false } };
export default function UnsubscribePage() {
  return <div className="settings-page single"><div className="settings-body">
    <h1>Go-live emails</h1>
    <section className="panel section"><Unsubscribe /></section>
  </div></div>;
}
