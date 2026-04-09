import init, {
  press_up,
  press_down,
  press_left,
  press_right,
  press_cancel,
  press_confirm,
  press_power,
  upload_epub,
} from "./pkg/web_simulator.js";

const actions = {
  up: press_up,
  down: press_down,
  left: press_left,
  right: press_right,
  cancel: press_cancel,
  confirm: press_confirm,
  power: press_power,
};

function fitCanvas() {
  const wrap = document.getElementById("screen-wrap");
  const canvas = document.getElementById("screen");
  const maxW = wrap.clientWidth;
  const maxH = wrap.clientHeight;
  const ratio = canvas.width / canvas.height;

  let w = maxW;
  let h = Math.floor(w / ratio);
  if (h > maxH) {
    h = maxH;
    w = Math.floor(h * ratio);
  }
  canvas.style.width = `${w}px`;
  canvas.style.height = `${h}px`;
}

async function main() {
  await init();

  document.querySelectorAll("button[data-action]").forEach((btn) => {
    btn.addEventListener("click", () => {
      const fn = actions[btn.dataset.action];
      if (fn) fn();
    });
  });

  document.getElementById("upload").addEventListener("change", async (event) => {
    const file = event.target.files?.[0];
    if (!file) return;
    const bytes = new Uint8Array(await file.arrayBuffer());
    upload_epub(file.name, bytes);
    event.target.value = "";
  });

  window.addEventListener("resize", fitCanvas);
  fitCanvas();
}

main();
