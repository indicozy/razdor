// Cursor sparkles: small gold and white pixels that trail the mouse. Pointer devices only, off with reduced
// motion and when the footer's switch is off (remembered in localStorage, try/catch).
const KEY = "sparks";
const toggle = document.querySelector<HTMLButtonElement>("[data-sparks]");
const fine = matchMedia("(pointer: fine)").matches;
const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;

function enabled(): boolean {
  try {
    return localStorage.getItem(KEY) !== "off";
  } catch {
    return true;
  }
}

let last = 0;
function spark(x: number, y: number) {
  const now = performance.now();
  if (now - last < 45) return;
  last = now;
  const s = document.createElement("i");
  s.className = "spark";
  s.style.left = `${x}px`;
  s.style.top = `${y}px`;
  s.style.background = Math.random() < 0.5 ? "#ffe39a" : "#ffffff";
  document.body.appendChild(s);
  setTimeout(() => s.remove(), 600);
}

function paint() {
  if (toggle) {
    const on = enabled();
    toggle.textContent = on ? toggle.dataset.off || "" : toggle.dataset.on || "";
    toggle.setAttribute("aria-pressed", String(on));
  }
}

if (fine && !reduced) {
  document.addEventListener("pointermove", (e) => {
    if (enabled()) spark(e.clientX + window.scrollX, e.clientY + window.scrollY);
  });
}
toggle?.addEventListener("click", () => {
  const on = enabled();
  try { localStorage.setItem(KEY, on ? "off" : "on"); } catch {}
  paint();
});
paint();
