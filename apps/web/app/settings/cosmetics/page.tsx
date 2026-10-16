import { redirect } from "next/navigation";

// Merged into the Persona editor (docs/PROFILES.md "Persona pass").
export default function Page() { redirect("/settings/profile#look"); }
