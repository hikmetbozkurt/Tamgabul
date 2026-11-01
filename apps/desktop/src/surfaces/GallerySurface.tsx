import { useState } from "react";
import { Icon } from "../ui/icons";
import { Flag } from "../ui/flags";
import { Dialog, MockBadge, Pill, Segmented, Skeleton, Status, Toggle } from "../ui/primitives";
import { useT } from "../i18n";
import { ResultCard } from "../features/search/ResultCard";

// Dev-only page (?surface=gallery): every component in every variant, the code
// counterpart of the "Tasarım sistemi" board on the design canvas.

const SWATCHES = [
  "bg", "surface", "sidebar", "chip", "nav-active", "bubble-user", "bubble-ai", "line",
  "ink", "muted", "primary", "accent", "file-tile", "spark",
];

export function GallerySurface() {
  const t = useT();
  const [on, setOn] = useState(true);
  const [seg, setSeg] = useState<"a" | "b">("a");
  const [dlg, setDlg] = useState(false);
  return (
    <main className="gallery">
      <section className="full" style={{ flexDirection: "row", alignItems: "baseline", gap: 20 }}>
        <h1 style={{ fontSize: 40, fontWeight: 800, letterSpacing: -0.8 }}>Tamgabul design system</h1>
        <span className="muted">tokens.css · ui/ · docs/24</span>
      </section>

      <section>
        <h2 className="section-label">Colors</h2>
        <div className="swatches">
          {SWATCHES.map((s) => (
            <span className="swatch" key={s}><i style={{ background: `var(--${s})` }} />{s}</span>
          ))}
        </div>
      </section>

      <section>
        <h2 className="section-label">Status (always with text)</h2>
        <Status tone="green">{t("status.READY")}</Status>
        <Status tone="amber">{t("status.AVAILABLE")}</Status>
        <Status tone="red">{t("status.INCOMPATIBLE")}</Status>
        <Status tone="neutral">{t("status.OPTIONAL")}</Status>
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
          <Pill tone="green">{t("status.READY")}</Pill><Pill tone="amber">{t("status.LOADING")}</Pill><Pill tone="red">{t("status.ERROR")}</Pill>
        </div>
        <div style={{ display: "flex", gap: 8 }}><span className="badge">Required</span><span className="badge badge-err">Not installed</span><MockBadge /></div>
      </section>

      <section>
        <h2 className="section-label">Type — Inter</h2>
        <div className="hero-title" style={{ textAlign: "left" }}>Tamgabul</div>
        <div className="page-title">Page title 28/700</div>
        <div className="result-title">Card title 19/700</div>
        <div style={{ fontSize: 17 }}>Body 17/400</div>
        <div className="muted">Secondary 15.5 · label 14.5 · small 13.5</div>
      </section>

      <section>
        <h2 className="section-label">Buttons</h2>
        <div style={{ display: "flex", gap: 10, flexWrap: "wrap", alignItems: "center" }}>
          <button className="btn btn-primary"><Icon name="external" size={17} stroke={1.8} />Open</button>
          <button className="btn"><Icon name="eye" size={18} />Preview</button>
          <button className="btn btn-danger">Remove &amp; purge</button>
          <button className="btn btn-primary" disabled>Disabled</button>
          <button className="btn-round" aria-label="Mic"><Icon name="mic" size={20} stroke={1.8} /></button>
          <button className="btn btn-icon" aria-label="More"><Icon name="more" size={16} /></button>
        </div>
        <button className="btn btn-soft" style={{ width: 280 }}><Icon name="plus" size={16} stroke={1.8} />Add model</button>
      </section>

      <section>
        <h2 className="section-label">Chips, toggle, segmented</h2>
        <div style={{ display: "flex", gap: 10, flexWrap: "wrap" }}>
          <button className="chip">Suggestion</button>
          <span className="chip chip-meta"><Icon name="clock" size={17} />2 days ago</span>
          <button className="chip chip-choice">20coin Clash <span className="count">· 2</span></button>
        </div>
        <div style={{ display: "flex", gap: 16, alignItems: "center" }}>
          <Toggle checked={on} onChange={setOn} label="Demo toggle" /> <span className="small muted">{on ? "On" : "Off"}</span>
        </div>
        <Segmented label="Demo" value={seg} onChange={setSeg} options={[
          { value: "a", label: <span className="lang-option"><Flag lang="en" />English</span> },
          { value: "b", label: <span className="lang-option"><Flag lang="tr" />Türkçe</span> },
        ]} />
      </section>

      <section>
        <h2 className="section-label">Inputs</h2>
        <form className="searchbar sb-follow" onSubmit={(e) => e.preventDefault()}>
          <span className="spark-tile"><Icon name="spark" size={18} color="var(--accent)" /></span>
          <input placeholder="Ask something else..." aria-label="Demo" />
        </form>
        <div className="field"><label htmlFor="g-in">Form field</label><input id="g-in" className="input" defaultValue="Ctrl + Shift + Space" /></div>
        <Skeleton h={18} w="70%" />
        <button className="btn btn-sm" onClick={() => setDlg(true)}>Open dialog</button>
      </section>

      <section className="full">
        <h2 className="section-label">Result card</h2>
        <div style={{ maxWidth: 710 }}>
          <ResultCard
            r={{
              result_ref: "demo", title: "auction-timer-bug.md", display_path: "C:/Projects/20coin-clash/docs/bugs",
              project: "20coin Clash", file_kind: "markdown", modified_at: new Date(Date.now() - 2 * 86_400_000).toISOString(),
              match_reason: "semantic", highlights: ["Auction timer"],
              snippet: "Auction timer sometimes drops below zero in the last 10 seconds.",
            }}
            onOpen={() => {}} onReveal={() => {}} onPreview={() => {}}
          />
        </div>
      </section>
      {dlg && (
        <Dialog title="Dialog" subtitle="Esc closes, focus stays inside" onClose={() => setDlg(false)} small>
          <div className="dialog-actions"><button className="btn" onClick={() => setDlg(false)}>Cancel</button><button className="btn btn-primary">OK</button></div>
        </Dialog>
      )}
    </main>
  );
}
