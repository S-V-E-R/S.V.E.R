"use client";
import Image from "next/image";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useState } from "react";
import { send } from "../lib/client-api";
import { Status, type SaveState } from "./Form";

/** Optional S.V.E.R badge (docs/PROFILES.md "Logo style"); "default" is the plain text logo. */
export type LogoStyle = "default" | "male" | "female";
const STYLES: [LogoStyle, string][] = [["default", "Default"], ["male", "Male"], ["female", "Female"]];

/** The top-bar logo: the chosen badge, if any, before the wordmark. */
export function Logo({ style, className = "logo" }: { style?: LogoStyle; className?: string }) {
  return <Link href="/" className={className} aria-label="S.V.E.R home">
    {style && style !== "default" && <Image src={`/logo/${style}.webp`} width={36} height={36} alt="" unoptimized className="logo-badge" />}
    S.V.E.R
  </Link>;
}

/** Picks the badge; saves on each change and refreshes the header. */
export function LogoStylePicker({ value }: { value: LogoStyle }) {
  const router = useRouter();
  const [current, setCurrent] = useState(value);
  const [state, setState] = useState<SaveState>({});
  async function pick(style: LogoStyle) {
    const before = current;
    setCurrent(style);
    const result = await send("PUT", "/api/me/logo-style", { logo_style: style });
    setState(result.ok ? { saved: "Logo saved." } : result);
    if (!result.ok) { setCurrent(before); return; }
    router.refresh();
  }
  return <fieldset className="logo-styles">
    <legend>S.V.E.R logo style <span className="muted">(optional)</span></legend>
    {STYLES.map(([style, label]) => <label key={style} className={current === style ? "logo-style chosen" : "logo-style"}>
      <input type="radio" className="sr-only" name="logo_style" value={style} checked={current === style} onChange={() => pick(style)} />
      {style === "default" ? <span className="logo small" aria-hidden="true">S.V.E.R</span> : <Image src={`/logo/${style}.webp`} width={72} height={72} alt="" unoptimized />}
      <span>{label}</span>
    </label>)}
    <Status state={state} />
  </fieldset>;
}
