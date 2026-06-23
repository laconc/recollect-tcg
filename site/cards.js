// Card-catalog filter — progressive enhancement for site/cards.html. With JS off,
// every card shows (the markup is fully rendered server-side). Externalised from an
// inline <script> so the site can ship a strict `script-src 'self'` CSP (#105): no
// inline script, no inline handler — the form's submit is cancelled here, not via an
// onsubmit attribute. Generated page; this file is hand-written and copied by `make
// site`. See tools/gen_cards_page.py.
(function () {
  var form = document.querySelector(".cards-toolbar");
  var q = document.getElementById("card-search");
  var selects = Array.prototype.slice.call(document.querySelectorAll("[data-filter]"));
  var cards = Array.prototype.slice.call(document.querySelectorAll(".card"));
  var count = document.getElementById("card-count");
  if (!q || !count) return;

  function apply() {
    var term = q.value.trim().toLowerCase();
    var shown = 0;
    cards.forEach(function (c) {
      var ok =
        (!term || c.dataset.name.indexOf(term) !== -1) &&
        selects.every(function (s) {
          return !s.value || c.dataset[s.dataset.filter] === s.value;
        });
      c.hidden = !ok;
      if (ok) shown++;
    });
    count.textContent = shown + " of " + cards.length;
  }

  // The search box is not a submit form (it filters live); cancel any Enter-submit.
  if (form) form.addEventListener("submit", function (e) { e.preventDefault(); });
  q.addEventListener("input", apply);
  selects.forEach(function (s) { s.addEventListener("change", apply); });
})();
