/** "May set my board" on the approval screens (/go and /oauth/authorize) of an app that connects as a CrowdSync game. Off unless ticked. */
export function MayDefine({ checked, onChange }: { checked: boolean; onChange: (on: boolean) => void }) {
  return <div>
    <label className="checkbox"><input type="checkbox" checked={checked} onChange={e => onChange(e.target.checked)} /> May set my board</label>
    <p className="muted small">Lets this game put its own board on your channel while it is connected. Your board comes back when it disconnects. You can change this any time in Settings → Connected apps.</p>
  </div>;
}
