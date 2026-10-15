/** Keep one decoder seek in flight and retain only the latest drag target. */
export function createScrubSeek(video: HTMLVideoElement) {
  let disposed = false;
  let busy = false;
  let pending: number | null = null;
  let latest: number | null = null;
  let lastWrite = -Infinity;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let watchdog: ReturnType<typeof setTimeout> | null = null;

  function clearTimers() {
    if (timer !== null) clearTimeout(timer);
    if (watchdog !== null) clearTimeout(watchdog);
    timer = watchdog = null;
  }

  function drain() {
    if (disposed || busy || pending === null || timer !== null) return;
    const delay = Math.max(0, 50 - (Date.now() - lastWrite));
    if (delay > 0) {
      timer = setTimeout(() => { timer = null; drain(); }, delay);
      return;
    }
    const target = pending;
    pending = null;
    if (Math.abs(video.currentTime - target) < 0.001) return;
    busy = true;
    lastWrite = Date.now();
    // Missing seeked events must not leave the drag pipeline locked forever.
    watchdog = setTimeout(settled, 800);
    try {
      video.currentTime = target;
    } catch {
      settled();
    }
  }

  function settled() {
    if (disposed) return;
    if (watchdog !== null) clearTimeout(watchdog);
    watchdog = null;
    busy = false;
    drain();
  }

  function cancel() {
    disposed = true;
    pending = null;
    clearTimers();
    video.removeEventListener('seeked', settled);
  }

  video.addEventListener('seeked', settled);
  return {
    request(target: number) {
      if (disposed || !Number.isFinite(target) || !Number.isFinite(video.duration) || video.duration <= 0) return;
      latest = pending = Math.max(0, Math.min(video.duration, target));
      drain();
    },
    finish() {
      if (disposed) return;
      const target = latest;
      cancel();
      // Release commits the final position immediately, even during a slow seek.
      if (target !== null && Math.abs(video.currentTime - target) >= 0.001) {
        try { video.currentTime = target; } catch { /* Media may have been unloaded. */ }
      }
    },
    cancel,
  };
}
