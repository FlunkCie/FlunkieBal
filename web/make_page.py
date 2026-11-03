"""Wrap the no-modules wasm-bindgen glue into a single page (published as an artifact)."""
import sys
glue = open(sys.argv[1]).read()
assert '</script' not in glue
page = '''<title>FlunkieBal</title>
<meta name="theme-color" content="#12201a">
<style>
  /* One-screen game: a full-bleed canvas with a loading card on top. Deliberately dark-only. */
  :root { --bg: #12201a; --bg-hi: #1d3a2b; --gold: #f2b632; --ink: #f4efe2; --track: rgba(244,239,226,.14); color-scheme: dark; }
  html, body { height: 100%; overflow: hidden; overscroll-behavior: none; }
  body { background: var(--bg); color: var(--ink); }
  canvas#game { position: fixed; inset: 0; width: 100%; height: 100%; display: block; touch-action: none; outline: none; -webkit-tap-highlight-color: transparent; }
  #boot { position: fixed; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 14px;
    background: radial-gradient(120% 80% at 50% 30%, var(--bg-hi) 0%, var(--bg) 70%); color: var(--ink);
    font: 600 15px/1.4 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif; transition: opacity .45s; z-index: 5; padding-inline: 16px; text-align: center; }
  #boot.done { opacity: 0; pointer-events: none; }
  #boot.crashed { opacity: 1; pointer-events: auto; cursor: pointer; background: rgba(12,10,8,.92); }
  #boot.crashed .bar { display: none; }
  #boot.crashed #boot-msg { white-space: pre-wrap; max-width: 520px; font-size: 14px; }
  #boot h1 { margin: 0; font: 900 clamp(42px, 13vw, 84px)/1 "Arial Black", Impact, sans-serif; letter-spacing: .02em; color: var(--gold); text-wrap: balance; }
  #boot .bar { width: min(240px, 60vw); height: 6px; border-radius: 3px; background: var(--track); overflow: hidden; }
  #boot .bar i { display: block; height: 100%; width: 40%; background: var(--gold); border-radius: 3px; animation: slide 1.1s ease-in-out infinite; }
  @keyframes slide { 0% { transform: translateX(-100%); } 100% { transform: translateX(250%); } }
  @media (prefers-reduced-motion: reduce) { #boot .bar i { animation: none; width: 100%; } }
  #boot-msg { opacity: .78; max-width: 32ch; }
</style>
<canvas id="game"></canvas>
<div id="boot"><h1>FLUNKIEBAL</h1><div class="bar"><i></i></div><div id="boot-msg">Loading the Vondelpark…</div></div>
<div id="safe-probe" style="position:fixed;left:0;top:0;visibility:hidden;pointer-events:none;padding:env(safe-area-inset-top,0px) env(safe-area-inset-right,0px) env(safe-area-inset-bottom,0px) env(safe-area-inset-left,0px)"></div>
<script>
  // safe area for the notch and the home bar: CSS env() and, in the Android app, the display cutout
  window.flunkiInsets = function () {
    var v = [0, 0, 0, 0];
    try { var cs = getComputedStyle(document.getElementById("safe-probe")); v = [cs.paddingTop, cs.paddingRight, cs.paddingBottom, cs.paddingLeft].map(parseFloat); } catch (e) {}
    try { if (window.FlunkiVoice && FlunkiVoice.insets) { var a = String(FlunkiVoice.insets()).split(",").map(Number); for (var i = 0; i < 4; i++) v[i] = Math.max(v[i] || 0, a[i] || 0); } } catch (e) {}
    return v.join(",");
  };
  // phones only show the keyboard when a text field gets focus during the tap itself
  if (typeof navigator.vibrate !== "function") { try { navigator.vibrate = function () { return false; }; } catch (e) {} }
  // an error thrown inside the game loop stops the frames: show the restart screen rather than a frozen picture
  window.addEventListener("error", function (e) {
    var st = (e.error && e.error.stack) || "";
    if (/wasm|RuntimeError|already borrowed|unreachable/.test(st + " " + e.message)) {
      var b = document.getElementById("boot"); if (b.className === "crashed") return;
      b.className = "crashed";
      document.getElementById("boot-msg").textContent = "Something went wrong and the game stopped. Tap to restart.\\n\\n" + String(e.message).slice(0, 300);
    }
  });
  document.getElementById("boot").addEventListener("click", function () { if (this.className === "crashed") location.reload(); });
  (function () {
    var cv = document.getElementById("game");
    function kb() { return window.FlunkiVoice && window.FlunkiVoice.showKeyboard ? window.FlunkiVoice : null; }
    var hideT = 0;
    function hook(inp) {
      if (inp.__fk) return; inp.__fk = 1;
      inp.addEventListener("focusout", function () { var b = kb(); if (!b) return; clearTimeout(hideT); hideT = setTimeout(function () { if (document.activeElement !== inp) b.hideKeyboard(); }, 250); });
      inp.addEventListener("focus", function () { clearTimeout(hideT); });
    }
    function tryFocus(x, y) {
      try {
        if (window.flunki_text_hit && window.flunki_text_hit(x, y)) {
          var inp = document.querySelector("input[type=text]");
          if (inp) {
            hook(inp);
            inp.style.left = Math.max(0, x - 1) + "px"; inp.style.top = Math.max(0, y - 1) + "px";
            inp.focus({ preventScroll: true });
            var b = kb(); if (b) b.showKeyboard();
            setTimeout(function () { if (document.activeElement !== inp) { inp.focus({ preventScroll: true }); if (b) b.showKeyboard(); } }, 60);
          }
        }
      } catch (e) {}
    }
    cv.addEventListener("touchend", function (e) { var t = e.changedTouches && e.changedTouches[0]; if (t) tryFocus(t.clientX, t.clientY); }, { passive: true });
    cv.addEventListener("mouseup", function (e) { tryFocus(e.clientX, e.clientY); });
  })();
</script>
<script>
''' + glue + '''
</script>
<script>
  wasm_bindgen({ module_or_path: "flunkiebal_bg.wasm" }).then(() => {
    window.flunki_cmd = wasm_bindgen.flunki_cmd; window.flunki_back = wasm_bindgen.flunki_back; window.flunki_text_hit = wasm_bindgen.flunki_text_hit;
  }).catch(e => { document.getElementById("boot-msg").textContent = "Could not load the game: " + e; });
</script>
'''
open(sys.argv[2], 'w').write(page)
