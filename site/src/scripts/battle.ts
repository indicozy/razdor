// The scripted battle of the landing (section E). Without JS the static field stays; with JS the enemy
// portraits become buttons and one fixed turn plays: three attacks, two answers, a victory on the third.
interface BattleStrings {
  turn: string;
  attack: string;
  hit: string;
  dead: string;
  victory: string;
  next: string;
  foes: string[];
  ours: string[];
}
interface Config {
  base: string;
  s: BattleStrings;
}

const root = document.querySelector<HTMLElement>("[data-battle]");
const configEl = root?.querySelector<HTMLScriptElement>("[data-battle-config]");

if (root && configEl?.textContent) {
  const cfg = JSON.parse(configEl.textContent) as Config;
  const S = cfg.s;
  const status = root.querySelector<HTMLElement>("[data-battle-status]");
  const enemyField = root.querySelector<HTMLElement>('[data-battle-field="enemy"]');
  const yourField = root.querySelector<HTMLElement>('[data-battle-field="you"]');
  const skip = root.querySelector<HTMLElement>("[data-battle-skip]");
  const next = root.querySelector<HTMLElement>("[data-battle-next]");
  const path = document.getElementById("path");
  const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  // The script's own numbers: the player's hits and the enemy's answers, in order.
  const playerDamage = [18, 26, 30];
  const enemyDamage = [14, 22];
  const playerAttackers = [0, 2, 4]; // front cards of the visitor's side
  const enemyAttackers = [2, 4]; // enemy portraits that answer
  const enemyTargets = [2, 5]; // the visitor's cards that get hit

  interface Foe {
    btn: HTMLButtonElement;
    bar: HTMLElement;
    veil: HTMLElement;
    name: string;
    hp: number;
    dead: boolean;
  }

  // The pauses stay under reduced motion: they are time to read the strip, not motion.
  const wait = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));
  const say = (text: string) => {
    if (status) status.textContent = text;
  };
  const line = (template: string, a: string, b: string, n: number) =>
    template.replace("{a}", a).replace("{b}", b).replace("{n}", String(n));

  // Enemy portraits become buttons; the markup inside (HP bar, veil) moves with them.
  const foes: Foe[] = [];
  enemyField?.querySelectorAll<HTMLElement>(".card.foe").forEach((div, i) => {
    const name = S.foes[i % S.foes.length];
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = div.className;
    btn.setAttribute("style", div.getAttribute("style") ?? "");
    btn.setAttribute("aria-label", S.attack.replace("{name}", name));
    btn.append(...Array.from(div.childNodes));
    div.replaceWith(btn);
    btn.classList.add("in");
    const foe: Foe = {
      btn,
      bar: btn.querySelector<HTMLElement>(".hp i") as HTMLElement,
      veil: btn.querySelector<HTMLElement>(".veil") as HTMLElement,
      name,
      hp: 100,
      dead: false,
    };
    btn.addEventListener("click", () => {
      if (busy || over || foe.dead) return;
      void attack(foes.indexOf(foe));
    });
    foes.push(foe);
  });
  const mine = Array.from(yourField?.querySelectorAll<HTMLElement>(".card.mine") ?? []);

  let busy = false;
  let over = false;
  let turn = 0;

  // Moves a card half-way toward its target and back (250 ms there).
  const slide = (el: HTMLElement, target: HTMLElement) => {
    if (reduced) return;
    const a = el.getBoundingClientRect();
    const b = target.getBoundingClientRect();
    const dx = (b.left - a.left) / 2;
    const dy = (b.top - a.top) / 2;
    el.animate(
      [
        { transform: "translate(0, 0)" },
        { transform: `translate(${dx}px, ${dy}px)`, offset: 0.5 },
        { transform: "translate(0, 0)" },
      ],
      { duration: 500, easing: "ease-in-out" },
    );
  };

  // The sprite sheet of the hit (25 frames) over the target card.
  const effect = (target: HTMLElement) => {
    if (reduced) return;
    const el = document.createElement("i");
    el.className = "hit";
    el.setAttribute("aria-hidden", "true");
    el.style.backgroundImage = `url('${cfg.base}art/kuski.webp')`;
    target.appendChild(el);
    setTimeout(() => el.remove(), 640);
  };

  const hurt = (foe: Foe, n: number) => {
    foe.hp = Math.max(0, foe.hp - n);
    foe.bar.style.width = `${foe.hp}%`;
  };

  const kill = (foe: Foe) => {
    foe.hp = 0;
    foe.dead = true;
    foe.bar.style.width = "0%";
    foe.veil.textContent = S.dead;
    foe.veil.classList.add("on");
    foe.btn.disabled = true;
  };

  const firstAlive = (from: number) => {
    for (let k = 0; k < foes.length; k++) {
      const f = foes[(from + k) % foes.length];
      if (!f.dead) return foes.indexOf(f);
    }
    return -1;
  };

  // The enemy answers with one portrait; it never kills.
  const answer = async (k: number) => {
    const fi = firstAlive(enemyAttackers[k]);
    if (fi < 0) return;
    const foe = foes[fi];
    const target = mine[enemyTargets[k]];
    slide(foe.btn, target);
    await wait(250);
    effect(target);
    say(line(S.hit, foe.name, S.ours[enemyTargets[k] % S.ours.length], enemyDamage[k]));
    await wait(600);
  };

  const attack = async (i: number) => {
    busy = true;
    const k = turn;
    const foe = foes[i];
    const attacker = mine[playerAttackers[k]];
    slide(attacker, foe.btn);
    await wait(250);
    effect(foe.btn);
    say(line(S.hit, S.ours[playerAttackers[k] % S.ours.length], foe.name, playerDamage[k]));
    hurt(foe, playerDamage[k]);
    turn++;

    if (k === 2) {
      // The third attack takes out the whole front row: the battle is won.
      kill(foe);
      for (const other of foes) {
        if (other === foe || other.dead) continue;
        await wait(150);
        kill(other);
      }
      over = true;
      say(S.victory);
      skip?.setAttribute("hidden", "");
      next?.removeAttribute("hidden");
      busy = false;
      return;
    }

    await wait(600);
    await answer(k);
    say(S.turn);
    busy = false;
  };

  if (status) status.textContent = S.turn;
  if (skip) {
    skip.addEventListener("click", (e) => {
      e.preventDefault();
      path?.scrollIntoView({ behavior: "smooth" });
    });
  }
  if (next) {
    next.addEventListener("click", (e) => {
      e.preventDefault();
      path?.scrollIntoView({ behavior: "smooth" });
    });
  }
}
