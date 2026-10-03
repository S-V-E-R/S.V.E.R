import { redirect } from "next/navigation";
import { currentAccount } from "./session";
export default async function Home() { redirect(await currentAccount() ? "/account" : "/login"); }
