import { el } from "./dom";

/** The owl (and its snail friend) in each mood. Colors come from `.m-*` classes in theme.css, so dark mode recolors them. */
export type Mood = "reading" | "checking" | "happy" | "confused" | "key" | "asleep" | "pen";

const EARS = '<path class="m-body" d="M30 34 34 8 54 26Z M90 34 86 8 66 26Z"/>';
const BEAK = '<path class="m-beak" d="M54 62h12l-6 9Z"/>';

function owl(eyes: string, extra = "", tall = true) {
  const body = tall
    ? '<rect class="m-body" x="24" y="18" width="72" height="92" rx="36"/><ellipse class="m-belly" cx="60" cy="84" rx="25" ry="22"/>'
    : '<rect class="m-body" x="24" y="18" width="72" height="86" rx="36"/><ellipse class="m-belly" cx="60" cy="80" rx="25" ry="20"/>';
  return EARS + body + eyes + BEAK + extra;
}

const OPEN_EYES = (dx = 0, dy = 0) =>
  `<g class="m-blink"><circle class="m-eye" cx="45" cy="48" r="15"/><circle class="m-eye" cx="75" cy="48" r="15"/>` +
  `<circle class="m-pupil" cx="${45 + dx}" cy="${49 + dy}" r="7"/><circle class="m-pupil" cx="${75 + dx}" cy="${49 + dy}" r="7"/></g>`;
const SMILE_EYES = '<path class="m-line" d="M34 52q11-12 22 0M64 52q11-12 22 0"/>';
const SHUT_EYES = '<path class="m-line" d="M34 48q11 8 22 0M64 48q11 8 22 0"/>';
const SPARK = (x: number, y: number, s: number, cls: string) =>
  `<path class="${cls}" d="M${x} ${y}l${s} ${2.3 * s} ${2.3 * s} ${s} -${2.3 * s} ${s} -${s} ${2.3 * s} -${s} -${2.3 * s} -${2.3 * s} -${s} ${2.3 * s} -${s}Z"/>`;

const ART: Record<Mood, { box: string; svg: string }> = {
  reading: {
    box: "0 0 120 120",
    svg: owl(OPEN_EYES(0, 4), '<path class="m-red" d="M28 96 60 90 92 96v16l-32-6-32 6Z"/><path class="m-page" d="M60 90v16"/>'),
  },
  checking: {
    box: "0 0 120 64",
    svg:
      '<rect class="m-trail m-grow" x="0" y="50" width="78" height="6" rx="3"/>' +
      '<g class="m-crawl"><path class="m-pink" d="M58 56q24-8 56-2v5H58Z"/><circle class="m-shell" cx="84" cy="34" r="20"/>' +
      '<path class="m-swirl" d="M84 34m-7 0a7 7 0 1 1 7 7a12 12 0 1 1-12-12"/><path class="m-feeler" d="M110 54l6-16"/><circle class="m-pupil" cx="116" cy="37" r="2.6"/></g>',
  },
  happy: {
    box: "0 0 150 120",
    svg:
      `<g transform="translate(15 0)">${owl(SMILE_EYES, '<circle class="m-pink" cx="36" cy="66" r="5"/><circle class="m-pink" cx="84" cy="66" r="5"/>')}</g>` +
      SPARK(14, 30, 3, "m-shell m-tw") + SPARK(132, 20, 3, "m-shell m-tw2") + SPARK(134, 78, 2, "m-pink m-tw"),
  },
  confused: {
    box: "0 0 144 120",
    svg:
      `<g class="m-tilt">${EARS}<rect class="m-body" x="24" y="18" width="72" height="92" rx="36"/><ellipse class="m-belly" cx="60" cy="84" rx="25" ry="22"/>` +
      '<circle class="m-eye" cx="45" cy="48" r="15"/><circle class="m-eye" cx="75" cy="48" r="11"/><circle class="m-pupil" cx="45" cy="48" r="4"/><circle class="m-pupil" cx="77" cy="46" r="6"/>' +
      '<path class="m-frown" d="M54 64q6-4 12 0"/></g>' +
      '<g class="m-pop"><circle class="m-red" cx="122" cy="26" r="17"/><path class="m-q" d="M116 21q0-7 6-7t6 6q0 4-6 7v4"/><circle class="m-page" cx="122" cy="37" r="2.2"/></g>',
  },
  key: {
    box: "0 0 136 120",
    svg: owl(OPEN_EYES(5, -3), '<g class="m-sway"><circle class="m-keyring" cx="112" cy="40" r="13"/><path class="m-keybit" d="M112 53v39M112 76h10M112 86h8"/></g>'),
  },
  asleep: {
    box: "0 0 152 120",
    svg: owl(SHUT_EYES, '<rect class="m-branch" x="2" y="104" width="134" height="8" rx="4"/><path class="m-leaf" d="M118 104q10-10 22-6"/>' +
      '<text class="m-z m-zz" x="104" y="34">z</text><text class="m-z m-zz2" x="116" y="20" font-size="15">z</text>', false),
  },
  pen: {
    box: "0 0 120 120",
    svg: owl(OPEN_EYES(4, 3), '<g class="m-wiggle"><rect class="m-shell" x="84" y="56" width="10" height="44" rx="3" transform="rotate(28 89 78)"/><path class="m-body" d="m96 98 6 12-12-6Z"/></g>'),
  },
};

/** A decorative mascot fitted in a `size`×`size` box; the text beside it says what it means. */
export function mascot(mood: Mood, size: number) {
  const { box, svg } = ART[mood];
  const [, , w, h] = box.split(" ").map(Number);
  const scale = size / Math.max(w, h);
  const s = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  s.setAttribute("viewBox", box);
  s.setAttribute("width", String(w * scale));
  s.setAttribute("height", String(h * scale));
  s.innerHTML = svg;
  return el("span", { className: `mascot m-${mood}`, ariaHidden: "true" }, s);
}
