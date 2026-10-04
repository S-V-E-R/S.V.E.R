"use client";
import { useCallback, useEffect, useState } from "react";
import { send, useLoad } from "../lib/client-api";
import Link from "next/link";

export function StaffRemovalAlerts() {
  const [count,setCount]=useState(0);
  const load=useCallback(async () => { const result=await send<{ urgent_take_down?:number }>("GET","/api/me/alerts"); if(result.ok) setCount(result.data.urgent_take_down || 0); },[]);
  useLoad(load);
  useEffect(() => { const timer=setInterval(() => { void load(); },30000); return () => clearInterval(timer); },[load]);
  return count>0 ? <Link className="button small" href="/admin/take-it-down" role="status">{count} urgent removal request{count===1 ? "" : "s"}</Link> : null;
}

export function EnableStaffPush() {
  const [message,setMessage]=useState("");
  const [busy,setBusy]=useState(false);
  async function enable() {
    setBusy(true);
    try {
      if (!("serviceWorker" in navigator) || !("PushManager" in window)) throw new Error("This browser does not support push. On iPhone or iPad, install S.V.E.R on your home screen first.");
      const config=await send<{ public_key:string }>("GET","/api/admin/push");
      if(!config.ok) throw new Error(config.error);
      if(!config.data.public_key) throw new Error("Staff push has not been configured.");
      if(await Notification.requestPermission()!=="granted") throw new Error("Allow notifications in your browser settings to receive urgent staff alerts.");
      const registration=await navigator.serviceWorker.register("/staff-push.js",{scope:"/"});
      await navigator.serviceWorker.ready;
      const encoded=config.data.public_key.replaceAll("-","+").replaceAll("_","/");
      const publicKey=Uint8Array.from(atob(encoded),char=>char.charCodeAt(0));
      const subscription=await registration.pushManager.getSubscription() || await registration.pushManager.subscribe({userVisibleOnly:true,applicationServerKey:publicKey});
      const saved=await send("POST","/api/admin/push",subscription.toJSON());
      if(!saved.ok) throw new Error(saved.error);
      setMessage("Urgent staff push alerts are enabled on this browser.");
    } catch(error) { setMessage(error instanceof Error ? error.message : "Push could not be enabled."); }
    setBusy(false);
  }
  return <div><button type="button" className="small quiet" disabled={busy} onClick={enable}>Enable staff push alerts</button>{message && <p role="status">{message}</p>}</div>;
}
