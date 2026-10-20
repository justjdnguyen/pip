'use strict';

// Site adapters, like Firefox's PiP "site wrappers". Each one knows how to read
// and toggle captions and how to go fullscreen for a given site.
//
// Captions controller interface:
//   available(): boolean        – whether the CC button should be enabled
//   enabled(): boolean          – whether captions are currently on
//   toggle(): void
//   onChange(cb(lines: string[])): void  – called with the current caption lines
//                                          (and whenever state changes)
//   destroy(): void

function stripCueMarkup(text) {
  return text
    .replace(/<[^>]*>/g, '')
    .replace(/&nbsp;/g, ' ')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&amp;/g, '&');
}

// Standard <track>/TextTrack captions.
function textTrackCaptions(video) {
  const isCaption = (t) => t.kind === 'subtitles' || t.kind === 'captions';
  const tracks = () => [...video.textTracks].filter(isCaption);
  const originalModes = new Map(tracks().map((t) => [t, t.mode]));
  let current = null;
  let userToggled = false;
  let listener = () => {};

  const emit = () => {
    const cues = current?.activeCues ? [...current.activeCues] : [];
    const lines = cues.flatMap((c) => stripCueMarkup(c.text || '').split('\n')).filter((l) => l.trim());
    listener(lines);
  };

  const attach = (track) => {
    current?.removeEventListener('cuechange', emit);
    current = track;
    if (track) {
      // 'hidden' still fires cuechange but stops the browser drawing them too.
      if (track.mode !== 'hidden') track.mode = 'hidden';
      track.addEventListener('cuechange', emit);
    }
    emit();
  };

  const pickDefault = () => {
    const list = tracks();
    const lang = (navigator.language || 'en').slice(0, 2).toLowerCase();
    return (
      list.find((t) => (t.language || '').toLowerCase().startsWith(lang)) ||
      list[0] ||
      null
    );
  };

  attach(tracks().find((t) => t.mode === 'showing') || tracks().find((t) => t.mode === 'hidden') || null);

  const onTracksChanged = () => emit();
  video.textTracks.addEventListener('addtrack', onTracksChanged);
  video.textTracks.addEventListener('removetrack', onTracksChanged);

  return {
    available: () => tracks().length > 0,
    enabled: () => !!current,
    toggle() {
      userToggled = true;
      if (current) {
        const t = current;
        attach(null);
        t.mode = 'disabled';
      } else {
        attach(pickDefault());
      }
    },
    onChange(cb) {
      listener = cb;
      emit();
    },
    destroy() {
      current?.removeEventListener('cuechange', emit);
      video.textTracks.removeEventListener('addtrack', onTracksChanged);
      video.textTracks.removeEventListener('removetrack', onTracksChanged);
      if (userToggled) {
        // Carry the user's choice back into the page.
        for (const t of tracks()) t.mode = t === current ? 'showing' : 'disabled';
      } else {
        for (const [t, mode] of originalModes) t.mode = mode;
      }
    },
  };
}

const genericAdapter = {
  name: 'generic',
  matches: () => true,
  capture: () => ({}),
  // Where the toggle button is anchored.
  rect: (video) => video.getBoundingClientRect(),
  // Where the "playing in PiP" placeholder goes. null = where the video was.
  placeholderSlot: () => null,
  captions: (video) => textTrackCaptions(video),
  fullscreen: (video) => video.requestFullscreen(),
};

// YouTube renders captions into its own DOM, not TextTracks, so we read the
// caption text from the player and toggle via YouTube's own CC button.
const youtubeAdapter = {
  name: 'youtube',
  matches: (video) =>
    /(^|\.)youtube(-nocookie)?\.com$/.test(location.hostname) && !!video.closest('.html5-video-player'),
  capture: (video) => ({ player: video.closest('.html5-video-player') }),
  // The <video> is letterboxed inside the player; anchor to the player like YouTube's own UI.
  rect: (video) => (video.closest('.html5-video-player') || video).getBoundingClientRect(),
  // The video's wrapper has no height of its own, so cover the whole player,
  // stacked just above the video layer and below captions/controls.
  placeholderSlot: (video, { player }) => {
    const container = video.closest('.html5-video-container');
    if (container?.parentNode !== player) return null;
    const z = parseInt(getComputedStyle(container).zIndex, 10);
    return { parent: player, before: container.nextSibling, zIndex: Number.isFinite(z) ? String(z + 1) : '' };
  },
  captions(video, { player }) {
    const button = () => player.querySelector('.ytp-subtitles-button');
    let listener = () => {};
    let lastKey = '';
    let lastEnabled = null;

    // Roll-up captions retain old lines above an overflow:hidden viewport.
    // textContent includes that history, even though YouTube no longer shows
    // it. Read the clipped text, including partially visible wrapped lines.
    const visibleText = (line, clip) => {
      const overlaps = (r) => r.width > 0 && r.height > 0 && r.bottom > clip.top + 0.5 && r.top < clip.bottom - 0.5;
      const inside = (r) => r.top >= clip.top - 0.5 && r.bottom <= clip.bottom + 0.5;
      const rect = line.getBoundingClientRect();
      if (!overlaps(rect)) return '';
      if (inside(rect)) return line.textContent;

      // A visual-line element can itself wrap onto several physical rows.
      // Use ranges only at the clipping boundary so hidden rows of that same
      // element don't leak into PiP either. No changes to YouTube's DOM.
      const range = document.createRange();
      const read = (node) => {
        if (node.nodeType !== 3) return [...node.childNodes].map(read).join('');
        const slice = (start, end) => {
          range.setStart(node, start);
          range.setEnd(node, end);
          const rects = [...range.getClientRects()];
          if (!rects.some(overlaps)) return '';
          if (rects.every(inside) || end - start <= 1) return node.textContent.slice(start, end);
          const mid = Math.floor((start + end) / 2);
          return slice(start, mid) + slice(mid, end);
        };
        return node.textContent ? slice(0, node.textContent.length) : '';
      };
      return read(line);
    };

    const readLines = () => {
      const lines = [];
      for (const win of player.querySelectorAll('.caption-window')) {
        const style = getComputedStyle(win);
        if (style.display === 'none' || style.visibility === 'hidden') continue;
        const clip = /hidden|clip|scroll|auto/.test(style.overflowY) ? win.getBoundingClientRect() : null;
        const visual = win.querySelectorAll('.caption-visual-line');
        if (visual.length) {
          for (const line of visual) lines.push(clip ? visibleText(line, clip) : line.textContent);
        } else {
          lines.push(clip ? visibleText(win, clip) : win.textContent);
        }
      }
      return lines.map((l) => l.replace(/\s+/g, ' ').trim()).filter(Boolean);
    };

    const enabled = () => button()?.getAttribute('aria-pressed') === 'true';

    // YouTube's DOM observer alone cannot help when its own renderer stops
    // in a hidden tab. Advance its queued animation work with media events,
    // which continue during background playback (also after seeks).
    const tick = () => {
      if (document.hidden && enabled()) {
        document.dispatchEvent(new Event('better-pip-caption-tick'));
      }
      check();
    };
    video.addEventListener('timeupdate', tick);
    video.addEventListener('seeked', tick);
    document.addEventListener('visibilitychange', tick);

    const check = () => {
      const lines = enabled() ? readLines() : [];
      const key = lines.join('\n');
      const en = enabled();
      if (key !== lastKey || en !== lastEnabled) {
        lastKey = key;
        lastEnabled = en;
        listener(lines);
      }
    };

    // Called directly (not via requestAnimationFrame): the tab is usually in
    // the background while PiP is open, and rAF doesn't run there.
    const observer = new MutationObserver(check);
    observer.observe(player, {
      subtree: true,
      childList: true,
      characterData: true,
      attributes: true,
      attributeFilter: ['aria-pressed', 'style', 'class', 'hidden'],
    });

    return {
      available() {
        const b = button();
        if (!b || getComputedStyle(b).display === 'none') return false;
        if (b.getAttribute('aria-disabled') === 'true') return false;
        const title = b.getAttribute('data-title-no-tooltip') || b.getAttribute('title') || '';
        return !/unavailable/i.test(title);
      },
      enabled,
      toggle: () => button()?.click(),
      onChange(cb) {
        listener = cb;
        lastKey = null;
        check();
      },
      destroy() {
        observer.disconnect();
        video.removeEventListener('timeupdate', tick);
        video.removeEventListener('seeked', tick);
        document.removeEventListener('visibilitychange', tick);
      },
    };
  },
  fullscreen(video, { player }) {
    const btn = player.querySelector('.ytp-fullscreen-button');
    if (btn) {
      btn.click();
      return Promise.resolve();
    }
    return player.requestFullscreen();
  },
};

BP.adapters = {
  list: [youtubeAdapter, genericAdapter],
  find(video) {
    return this.list.find((a) => a.matches(video));
  },
};
