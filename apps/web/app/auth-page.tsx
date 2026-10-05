import { redirect } from "next/navigation";
import { currentAccount } from "./session";
import AuthScreen from "./screens";

export type AuthScreenName = "login" | "signup" | "oauth-signup" | "forgot" | "reset" | "verify" | "mfa" | "account";
export type AuthSearchParams = Promise<{ error?: string | string[] }>;

// Shared by the explicit auth routes; behavior matches the former web/app/[screen] catch-all.
export async function authPage(screen: AuthScreenName, searchParams: AuthSearchParams) {
  if (["account", "login", "signup", "oauth-signup"].includes(screen)) {
    const account = await currentAccount();
    if (screen === "account" && !account) redirect("/login");
    if (screen !== "account" && account) {
      const error = (await searchParams).error;
      redirect(typeof error === "string" ? `/account?${new URLSearchParams({ error })}` : "/");
    }
  }
  return <AuthScreen screen={screen} />;
}
