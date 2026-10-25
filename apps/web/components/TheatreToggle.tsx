"use client";
import { useEffect, useRef } from "react";

const KEY = "sver-theatre";
/** Theatre mode on the watch page: hides the sidebar so the stage takes the width. Remembered per browser. */
export function TheatreToggle() {
  const button = useRef<HTMLButtonElement>(null);
  const apply = (on: boolean) => {
    button.current?.closest(".watch")?.classList.toggle("theatre", on);
    button.current?.setAttribute("aria-pressed", String(on));
  };
  useEffect(() => { try { if (localStorage.getItem(KEY) === "on") apply(true); } catch { /* storage blocked: default view */ } }, []);
  return <button ref={button} type="button" className="small quiet theatre-toggle" aria-pressed="false" onClick={() => {
    const on = !button.current?.closest(".watch")?.classList.contains("theatre");
    apply(on);
    try { localStorage.setItem(KEY, on ? "on" : "off"); } catch { /* this page only */ }
  }}>Theatre</button>;
}
