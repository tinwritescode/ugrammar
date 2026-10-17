import { proofy } from "./bridge";

document.addEventListener("mousedown", (e) => { if (e.button === 0) proofy.underlineClick(); });
