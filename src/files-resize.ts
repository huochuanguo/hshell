export function bindFilesResize(handle: HTMLElement, pane: HTMLElement) {
  const workspace = pane.parentElement!;
  let min = 0;
  let max = 0;
  let drag: { pointer: number; x: number; width: number } | undefined;

  const observer = new ResizeObserver(() => {
    const available = workspace.clientWidth;
    const half = Math.floor(available / 2);
    min = Math.min(280, half);
    max = Math.min(760, Math.max(half, available - 336));
    pane.style.setProperty("--files-min-width", `${min}px`);
    pane.style.setProperty("--files-max-width", `${max}px`);
    if (pane.hidden) return;
    handle.setAttribute("aria-valuemin", String(min));
    handle.setAttribute("aria-valuemax", String(max));
    const width = pane.offsetWidth;
    handle.setAttribute("aria-valuenow", String(width));
    handle.setAttribute("aria-valuetext", `${width} 像素`);
  });
  observer.observe(workspace);
  observer.observe(pane);

  const resize = (width: number) => {
    pane.style.setProperty(
      "--files-width",
      `${Math.max(min, Math.min(max, width))}px`,
    );
  };
  handle.addEventListener("pointerdown", (event) => {
    if (!event.isPrimary || event.button !== 0 || drag) return;
    drag = {
      pointer: event.pointerId,
      x: event.clientX,
      width: pane.offsetWidth,
    };
    handle.setPointerCapture(event.pointerId);
    handle.classList.add("dragging");
  });
  handle.addEventListener("pointermove", (event) => {
    if (drag?.pointer === event.pointerId)
      resize(drag.width + drag.x - event.clientX);
  });
  handle.addEventListener("lostpointercapture", (event) => {
    if (drag?.pointer !== event.pointerId) return;
    drag = undefined;
    handle.classList.remove("dragging");
  });
  handle.addEventListener("keydown", (event) => {
    const step = event.shiftKey ? 48 : 16;
    const width = pane.offsetWidth;
    const next = {
      ArrowLeft: width + step,
      ArrowRight: width - step,
      Home: min,
      End: max,
    }[event.key];
    if (next === undefined) return;
    event.preventDefault();
    resize(next);
  });
}
