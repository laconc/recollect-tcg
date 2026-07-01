// Site Options disclosure — the right-aligned settings control mirrored from the play
// header (recollect-web). Toggles the panel and persists the one site-wide setting:
// reduced motion. Progressive enhancement — with JS off the panel stays hidden and the
// OS `prefers-reduced-motion` media query (brand.css) still applies, so nothing is lost.
(function () {
  "use strict";
  var KEY = "recollect:reduce-motion";
  var root = document.documentElement;

  // Apply the saved preference immediately (before paint where possible).
  try {
    if (localStorage.getItem(KEY) === "1") root.classList.add("reduce-motion");
  } catch (e) {}

  function ready(fn) {
    if (document.readyState !== "loading") fn();
    else document.addEventListener("DOMContentLoaded", fn);
  }

  ready(function () {
    var btn = document.getElementById("options-toggle");
    var panel = document.getElementById("options-panel");
    if (!btn || !panel) return;

    function open() { panel.hidden = false; btn.setAttribute("aria-expanded", "true"); }
    function close() { panel.hidden = true; btn.setAttribute("aria-expanded", "false"); }

    btn.addEventListener("click", function (e) {
      e.stopPropagation();
      if (panel.hidden) open(); else close();
    });
    // Click-away and Escape close the panel.
    document.addEventListener("click", function (e) {
      if (!panel.hidden && !panel.contains(e.target) && e.target !== btn) close();
    });
    document.addEventListener("keydown", function (e) {
      if (e.key === "Escape" && !panel.hidden) { close(); btn.focus(); }
    });

    var rm = document.getElementById("opt-reduced-motion");
    if (rm) {
      rm.checked = root.classList.contains("reduce-motion");
      rm.addEventListener("change", function () {
        root.classList.toggle("reduce-motion", rm.checked);
        try { localStorage.setItem(KEY, rm.checked ? "1" : "0"); } catch (e) {}
      });
    }
  });
})();
