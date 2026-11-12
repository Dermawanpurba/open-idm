// Multi-connection segment geometry for the downloads table and the connection detail panel.
(function () {
  "use strict";

  const clamp = (value, min, max) => Math.min(max, Math.max(min, value));
  const round = value => Math.round(value * 100) / 100;

  function sorted(item) {
    return (item && Array.isArray(item.segments) ? item.segments.slice() : []).sort((a, b) => a.start - b.start);
  }

  function sizeOf(segment) {
    return Math.max(0, segment.end - segment.start + 1);
  }

  /** Downloaded part of each segment as a block of the progress bar (left/width in %). */
  function blocks(item) {
    const total = Number(item && item.filesize) || 0;
    if (total <= 0) return [];
    return sorted(item).map(segment => {
      const left = clamp((segment.start / total) * 100, 0, 100);
      const done = Math.min(segment.done, sizeOf(segment));
      const width = clamp((done / total) * 100, 0, 100 - left);
      return { left: round(left), width: round(width), active: Boolean(segment.active) };
    });
  }

  /** One row per segment for the connection list, in file order. */
  function rows(item) {
    return sorted(item).map((segment, index) => {
      const size = sizeOf(segment);
      const done = Math.min(segment.done, size);
      const state = done >= size ? "done" : segment.active ? "active" : "waiting";
      return {
        number: index + 1,
        start: segment.start,
        end: segment.end,
        size,
        done,
        percent: size > 0 ? Math.round((done / size) * 100) : 0,
        speed: segment.active ? Number(segment.speed) || 0 : 0,
        state,
      };
    });
  }

  function summary(item) {
    const list = sorted(item);
    const active = list.filter(segment => segment.active);
    return {
      segments: list.length,
      active: active.length,
      speed: active.reduce((sum, segment) => sum + (Number(segment.speed) || 0), 0),
    };
  }

  window.OpenIDMSegments = { blocks, rows, summary };
})();
