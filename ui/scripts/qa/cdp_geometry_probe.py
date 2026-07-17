import asyncio, json, sys, urllib.request, glob
for p in glob.glob("/tmp/oly-qa/venv/lib/python3.*/site-packages"): sys.path.insert(0, p)
import websockets
CDP = "http://127.0.0.1:9666"
_id = 0
def nxt():
    global _id; _id += 1; return _id
async def cmd(ws, m, p=None):
    i = nxt(); msg = {"id": i, "method": m}
    if p: msg["params"] = p
    await ws.send(json.dumps(msg))
    while True:
        r = json.loads(await ws.recv())
        if r.get("id") == i:
            if "error" in r: raise RuntimeError(m + str(r))
            return r.get("result", {})
async def ev(ws, e):
    r = await cmd(ws, "Runtime.evaluate", {"expression": e, "returnByValue": True, "awaitPromise": True})
    res = r["result"]
    if res.get("subtype") == "error": return {"JS_ERROR": res.get("description","")[:400]}
    return res.get("value")
async def main():
    tabs = json.load(urllib.request.urlopen(f"{CDP}/json/list"))
    page = next(t for t in tabs if t["type"] == "page")
    async with websockets.connect(page["webSocketDebuggerUrl"], max_size=50_000_000) as ws:
        await cmd(ws, "Page.enable"); await cmd(ws, "Runtime.enable")
        # emulate the user viewport
        await cmd(ws, "Emulation.setDeviceMetricsOverride", {"width": 1913, "height": 1370, "deviceScaleFactor": 1, "mobile": False})
        await cmd(ws, "Page.navigate", {"url": "http://127.0.0.1:5177/sessions"})
        await asyncio.sleep(4)
        await ev(ws, "document.querySelectorAll(\".srow\")[0]?.click()")
        await asyncio.sleep(2)
        print(await ev(ws, """
          (() => {
            const q = (s) => { const el = document.querySelector(s); if (!el) return null; const r = el.getBoundingClientRect(); return {h: Math.round(r.height), y: Math.round(r.y), bottom: Math.round(r.bottom)}; };
            return {
              win: {h: innerHeight},
              shell: q(".sessions-dock-shell"),
              group: q(".dv-groupview:not(:has(.dv-watermark))") || q(".dv-groupview"),
              content: q(".dv-content-container"),
              chatview: q(".chat-view"),
              chatcol: q(".chatcol"),
              transcript: q(".transcript"),
              bpanel: q(".bpanel"),
              groups: document.querySelectorAll(".sessions-dock-shell .dv-groupview").length,
            };
          })()
        """))
asyncio.run(main())
