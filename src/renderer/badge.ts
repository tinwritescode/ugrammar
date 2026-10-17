import { proofy } from "./bridge";

document.addEventListener("mousedown", (e) => { if (e.button === 0) proofy.badgeClick(); });
document.addEventListener("contextmenu", (e) => { e.preventDefault(); proofy.badgeMenu(); });
