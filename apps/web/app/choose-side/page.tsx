import { redirect } from "next/navigation";

// The faction step now lives in the onboarding wizard; this path stays for old links.
export default function ChooseSide() {
  redirect("/welcome");
}
