#!/usr/bin/env python3
"""Writes a before-and-after review page for packaging/render-shots.sh output.

    packaging/render-review.py --before BASE --after CANDIDATE --out review.html

Each folder holds THEME/SIZE/NAME.png. The page has a state selector with
Previous and Next, theme and size selectors, and Before and After buttons
(B and A, or Space to flip; arrow keys step through states). Pictures are
linked relative to the page, not copied, so keep the folders next to it.
Either folder may be left out for a page of one set.
"""

import argparse
import html
import json
import os
import sys
from pathlib import Path


def shots(folder):
    """{(theme, size, name): path} for every THEME/SIZE/NAME.png under folder."""
    found = {}
    if folder is None:
        return found
    for path in sorted(Path(folder).glob("*/*/*.png")):
        size, theme = path.parent.name, path.parent.parent.name
        found[(theme, size, path.stem)] = path
    return found


def size_order(size):
    """Sorts `WIDTHxHEIGHT` folders narrowest first, anything else after."""
    try:
        width, height = (int(part) for part in size.split("x"))
        return (0, width, height, size)
    except ValueError:
        return (1, 0, 0, size)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--before", type=Path, help="baseline captures")
    parser.add_argument("--after", type=Path, help="candidate captures")
    parser.add_argument("--out", type=Path, required=True, help="HTML file to write")
    parser.add_argument("--title", default="Visual review")
    parser.add_argument("--before-label", default="Before")
    parser.add_argument("--after-label", default="After")
    args = parser.parse_args()
    if args.before is None and args.after is None:
        parser.error("give --before, --after, or both")

    before, after = shots(args.before), shots(args.after)
    keys = sorted(set(before) | set(after))
    if not keys:
        sys.exit("no THEME/SIZE/NAME.png captures found")
    base = args.out.resolve().parent

    def link(path):
        return None if path is None else os.path.relpath(path.resolve(), base)

    data = {
        "names": sorted({name for _, _, name in keys}),
        "themes": sorted({theme for theme, _, _ in keys}),
        "sizes": sorted({size for _, size, _ in keys}, key=size_order),
        "shots": {
            "/".join(key): {"before": link(before.get(key)), "after": link(after.get(key))}
            for key in keys
        },
        "labels": {"before": args.before_label, "after": args.after_label},
        "default": "after" if after else "before",
    }
    page = TEMPLATE.replace("__TITLE__", html.escape(args.title)).replace(
        "__DATA__", json.dumps(data).replace("</", "<\\/")
    )
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(page, encoding="utf-8")
    print(f"wrote {args.out} ({len(keys)} captures)")


TEMPLATE = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>__TITLE__</title>
<style>
  :root { color-scheme: light dark; --bg: #f4f4f5; --fg: #18181b; --muted: #71717a;
          --panel: #ffffff; --line: #d4d4d8; --on: #1db954; --on-fg: #000; }
  @media (prefers-color-scheme: dark) {
    :root { --bg: #111113; --fg: #f4f4f5; --muted: #a1a1aa; --panel: #1c1c1f; --line: #3f3f46; }
  }
  * { box-sizing: border-box; }
  body { margin: 0; background: var(--bg); color: var(--fg);
         font: 14px/1.4 system-ui, -apple-system, "Segoe UI", sans-serif; }
  header { position: sticky; top: 0; z-index: 1; display: flex; flex-wrap: wrap; gap: 8px 16px;
           align-items: center; padding: 12px 16px; background: var(--panel);
           border-bottom: 1px solid var(--line); }
  h1 { font-size: 16px; margin: 0 8px 0 0; }
  .group { display: flex; gap: 4px; align-items: center; }
  .group span { color: var(--muted); margin-right: 4px; }
  button, select { font: inherit; color: inherit; background: var(--bg); border: 1px solid var(--line);
                   border-radius: 6px; padding: 4px 10px; cursor: pointer; }
  button[aria-pressed="true"] { background: var(--on); color: var(--on-fg); border-color: var(--on); }
  button:disabled { opacity: .4; cursor: default; }
  main { padding: 16px; }
  figure { margin: 0; }
  figcaption { color: var(--muted); margin-bottom: 8px; }
  img { display: block; max-width: 100%; height: auto; border: 1px solid var(--line); }
  .missing { padding: 48px; text-align: center; color: var(--muted); border: 1px dashed var(--line); }
</style>
</head>
<body>
<header>
  <h1>__TITLE__</h1>
  <div class="group"><button id="prev" title="Previous (Left)">&larr;</button>
    <select id="name" aria-label="State"></select>
    <button id="next" title="Next (Right)">&rarr;</button></div>
  <div class="group"><span>Theme</span><select id="theme" aria-label="Theme"></select></div>
  <div class="group"><span>Size</span><select id="size" aria-label="Size"></select></div>
  <div class="group" role="group" aria-label="Version">
    <button id="before" title="B">Before</button><button id="after" title="A">After</button></div>
</header>
<main><figure><figcaption id="caption"></figcaption><div id="view"></div></figure></main>
<script>
const data = __DATA__;
const $ = (id) => document.getElementById(id);
const state = { name: data.names[0], theme: data.themes.includes("dark") ? "dark" : data.themes[0],
                size: data.sizes[0], side: data.default };
try {
  // A link keeps the state it was copied in, where that state still exists.
  const saved = JSON.parse(decodeURIComponent(location.hash.slice(1)) || "{}");
  for (const [key, values] of [["name", data.names], ["theme", data.themes], ["size", data.sizes],
                               ["side", ["before", "after"]]]) {
    if (values.includes(saved[key])) state[key] = saved[key];
  }
} catch (e) {}
function fill(select, values) {
  for (const value of values) select.add(new Option(value, value));
}
fill($("name"), data.names); fill($("theme"), data.themes); fill($("size"), data.sizes);
$("before").textContent = data.labels.before; $("after").textContent = data.labels.after;
function shot() { return data.shots[[state.theme, state.size, state.name].join("/")] || {}; }
function show() {
  const index = data.names.indexOf(state.name);
  $("name").value = state.name; $("theme").value = state.theme; $("size").value = state.size;
  $("prev").disabled = index <= 0; $("next").disabled = index >= data.names.length - 1;
  const current = shot();
  for (const side of ["before", "after"]) {
    $(side).setAttribute("aria-pressed", String(state.side === side));
    $(side).disabled = !current[side];
  }
  const src = current[state.side];
  $("caption").textContent = `${state.name}, ${state.theme}, ${state.size}: ${data.labels[state.side]}`;
  $("view").innerHTML = "";
  if (src) {
    const img = new Image(); img.src = src; img.alt = $("caption").textContent; $("view").append(img);
  } else {
    const missing = document.createElement("div"); missing.className = "missing";
    missing.textContent = `No ${data.labels[state.side]} capture for this state.`; $("view").append(missing);
  }
  history.replaceState(null, "", "#" + encodeURIComponent(JSON.stringify(state)));
  // Load the other side too, so flipping is instant.
  const other = current[state.side === "before" ? "after" : "before"];
  if (other) new Image().src = other;
}
function step(by) {
  const index = data.names.indexOf(state.name) + by;
  if (index >= 0 && index < data.names.length) { state.name = data.names[index]; show(); }
}
$("name").onchange = (e) => { state.name = e.target.value; show(); };
$("theme").onchange = (e) => { state.theme = e.target.value; show(); };
$("size").onchange = (e) => { state.size = e.target.value; show(); };
$("prev").onclick = () => step(-1); $("next").onclick = () => step(1);
$("before").onclick = () => { state.side = "before"; show(); };
$("after").onclick = () => { state.side = "after"; show(); };
document.addEventListener("keydown", (e) => {
  if (e.target.tagName === "SELECT" || e.metaKey || e.ctrlKey || e.altKey) return;
  const current = shot();
  if (e.key === "ArrowLeft") step(-1);
  else if (e.key === "ArrowRight") step(1);
  else if (e.key === "b" && current.before) { state.side = "before"; show(); }
  else if (e.key === "a" && current.after) { state.side = "after"; show(); }
  else if (e.key === " ") {
    const other = state.side === "before" ? "after" : "before";
    if (current[other]) { state.side = other; show(); }
    e.preventDefault();
  }
});
show();
</script>
</body>
</html>
"""

if __name__ == "__main__":
    main()
