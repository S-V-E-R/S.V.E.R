"use client";
import Link from "next/link";
import { useState } from "react";
import { Identity, Images, Links, useMe } from "../profile-settings";
import { PersonaPreview } from "../../../../components/persona/PersonaPreview";
import { CurrentlyEditor, ThemeEditor } from "../../../../components/persona/PersonaFields";
import { SongEditor } from "../../../../components/persona/SongEditor";
import { WarCouncilEditor } from "../../../../components/persona/WarCouncilEditor";

const STEPS = ["Look", "Words", "Sound and people", "Links"] as const;

/**
 * First-time setup for your page (docs/PROFILES.md "Persona pass"): four steps, each skippable,
 * writing through the same editors as Settings → Your page, with the header preview alongside.
 */
export default function PersonaSetup() {
  const { me, error, refresh, version, bump } = useMe();
  const [step, setStep] = useState(0);
  if (!me) return <p className="loading">{error || "Loading…"}</p>;
  const last = step === STEPS.length - 1;
  const go = (n: number) => { setStep(n); window.scrollTo({ top: 0 }); };
  return <div className="persona-editor setup">
    <div className="persona-main">
      <header className="page-head"><span className="eyebrow">Set up your page · Step {step + 1} of {STEPS.length}</span><h1>{STEPS[step]}</h1>
        <p className="lead">{["Your picture, your banner and your colors.", "Who you are, in a few words, and what you're into right now.", "A profile song, and the eight people you'd put on your War Council.", "Where else people can find you."][step]}</p></header>
      <ol className="setup-steps" aria-label="Steps">{STEPS.map((s, i) => <li key={s} aria-current={i === step ? "step" : undefined} className={i < step ? "done" : undefined}><button type="button" className="link-button" onClick={() => go(i)}>{i + 1}. {s}</button></li>)}</ol>
      {step === 0 && <><Images me={me} onSaved={refresh} /><ThemeEditor onSaved={bump} /></>}
      {step === 1 && <><Identity me={me} onSaved={refresh} /><CurrentlyEditor onSaved={bump} /></>}
      {step === 2 && <><SongEditor embedded onSaved={bump} /><WarCouncilEditor embedded onSaved={bump} /></>}
      {step === 3 && <Links me={me} onSaved={refresh} />}
      <div className="row setup-nav">
        {step > 0 && <button type="button" className="quiet" onClick={() => go(step - 1)}>Back</button>}
        {last ? <Link className="button" href={`/${me.username}`}>Done: open your page</Link> : <button type="button" onClick={() => go(step + 1)}>Next</button>}
        {!last && <button type="button" className="link-button" onClick={() => go(step + 1)}>Skip this step</button>}
      </div>
    </div>
    <PersonaPreview username={me.username} version={version} />
  </div>;
}
