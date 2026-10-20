'use strict';

// YouTube's caption renderer uses the page's animation clock. Chrome suspends
// that clock in background tabs, even while our companion captures the video.
// Run in MAIN at document_start so YouTube cannot cache the unwrapped API.
// The isolated caption adapter sends ticks only while it owns a PiP session
// with captions enabled. No timer or visibility override is needed.
(() => {
  const request = window.requestAnimationFrame.bind(window);
  const cancel = window.cancelAnimationFrame.bind(window);
  const pending = new Map();

  window.requestAnimationFrame = (callback) => {
    // Leave the native API responsible for validating its argument.
    if (typeof callback !== 'function') return request(callback);
    const id = request((time) => {
      pending.delete(id);
      callback.call(window, time);
    });
    pending.set(id, callback);
    return id;
  };
  window.cancelAnimationFrame = (id) => {
    pending.delete(Number(id));
    cancel(id);
  };

  document.addEventListener('better-pip-caption-tick', () => {
    if (!document.hidden) return;
    const time = performance.now();
    // Snapshot first: callbacks scheduling another frame wait for the next
    // media tick. Keep cancellation within this batch working too.
    for (const [id, callback] of [...pending]) {
      if (!pending.has(id)) continue;
      pending.delete(id);
      cancel(id);
      try {
        callback.call(window, time);
      } catch (err) {
        window.reportError(err);
      }
    }
  });
})();
