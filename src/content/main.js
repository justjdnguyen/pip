'use strict';

(() => {
  if (BP.started) return;
  BP.started = true;

  BP.settings.load();
  BP.toggle = BP.createToggle();

  const isTop = window.top === window;

  // Alt+P (Option+P on macOS) toggles PiP for the main video on the page.
  // Handled in the page (not as a browser command) so the key press counts as
  // the user gesture that opening a PiP window requires.
  addEventListener(
    'keydown',
    (e) => {
      if (e.code !== 'KeyP' || !e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
      const handled = BP.Pip.toggleBest();
      if (handled) {
        e.preventDefault();
        e.stopPropagation();
      }
    },
    { capture: true },
  );

  chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
    if (msg?.type === 'toggle-pip' && isTop) {
      sendResponse({ handled: BP.Pip.toggleBest() });
    } else if (msg?.type === 'toggle-pip-frames' && !isTop) {
      BP.Pip.toggleBest();
    }
  });
})();
