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

function setStatus(message, tone = "info") {
  const status = document.getElementById("status");
  status.textContent = message;
  status.dataset.tone = tone;
}

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
  setStatus("Ready. Upload an EPUB to add it to the simulated SD card.");

  document.querySelectorAll("button[data-action]").forEach((btn) => {
    btn.addEventListener("click", () => {
      const fn = actions[btn.dataset.action];
      if (fn) fn();
    });
  });

  document.getElementById("upload").addEventListener("change", async (event) => {
    const file = event.target.files?.[0];
    if (!file) return;
    setStatus(`Uploading ${file.name}...`);
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      upload_epub(file.name, bytes);
      setStatus(`Uploaded ${file.name}.`);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setStatus(`Upload failed: ${message}`, "error");
    } finally {
      event.target.value = "";
    }
  });

  window.addEventListener("resize", fitCanvas);
  fitCanvas();
}

main();
