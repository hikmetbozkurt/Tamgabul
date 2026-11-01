import { useId } from "react";
import type { Language } from "../bindings/Language";

// Inline SVG flags: Windows does not render flag emoji, so we draw them.

function FlagTR() {
  return (
    <svg viewBox="0 0 1200 800" aria-hidden="true">
      <rect width="1200" height="800" fill="#E30A17" />
      <circle cx="425" cy="400" r="200" fill="#FFFFFF" />
      <circle cx="475" cy="400" r="160" fill="#E30A17" />
      <path fill="#FFFFFF" d="M583.334 400 764.235 458.779 652.431 304.894V495.106L764.235 341.221Z" />
    </svg>
  );
}

function FlagGB() {
  const id = useId().replace(/:/g, "");
  return (
    <svg viewBox="0 0 60 30" preserveAspectRatio="xMidYMid slice" aria-hidden="true">
      <clipPath id={`s${id}`}><path d="M0,0 v30 h60 v-30 z" /></clipPath>
      <clipPath id={`t${id}`}><path d="M30,15 h30 v15 z v15 h-30 z h-30 v-15 z v-15 h30 z" /></clipPath>
      <g clipPath={`url(#s${id})`}>
        <path d="M0,0 v30 h60 v-30 z" fill="#012169" />
        <path d="M0,0 L60,30 M60,0 L0,30" stroke="#FFFFFF" strokeWidth="6" />
        <path d="M0,0 L60,30 M60,0 L0,30" clipPath={`url(#t${id})`} stroke="#C8102E" strokeWidth="4" />
        <path d="M30,0 v30 M0,15 h60" stroke="#FFFFFF" strokeWidth="10" />
        <path d="M30,0 v30 M0,15 h60" stroke="#C8102E" strokeWidth="6" />
      </g>
    </svg>
  );
}

export function Flag({ lang }: { lang: Language }) {
  return <span className="flag">{lang === "tr" ? <FlagTR /> : <FlagGB />}</span>;
}
