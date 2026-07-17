import asyncio, base64, json, urllib.request, glob, sys
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
async def mouse(ws, t, x, y, buttons=1):
    await cmd(ws, "Input.dispatchMouseEvent", {"type": t, "x": x, "y": y, "button": "left", "buttons": buttons, "clickCount": 1 if t in ("mousePressed","mouseReleased") else 0})
async def main():
    tabs = json.load(urllib.request.urlopen(f"{CDP}/json/list"))
    page = next(t for t in tabs if t["type"] == "page")
    async with websockets.connect(page["webSocketDebuggerUrl"], max_size=50_000_000) as ws:
        await cmd(ws, "Page.enable"); await cmd(ws, "Runtime.enable")
        s = await ev(ws, "(() => { const sash = document.querySelector(\".sessions-dock-shell .dv-sash\"); if (!sash) return null; const r = sash.getBoundingClientRect(); return {x: Math.round(r.x+r.width/2), y: Math.round(r.y+r.height/2), w: r.width}; })()")
        w0 = await ev(ws, "Math.round(document.querySelector(\".sessions-dock-shell .dv-groupview\").getBoundingClientRect().width)")
        print("sash:", s, "group0 w:", w0)
        if s:
            await mouse(ws, "mousePressed", s["x"], s["y"])
            for i in range(1, 9):
                await mouse(ws, "mouseMoved", s["x"] - i*25, s["y"])
                await asyncio.sleep(0.03)
            await mouse(ws, "mouseReleased", s["x"] - 200, s["y"], buttons=0)
            await asyncio.sleep(0.5)
            w1 = await ev(ws, "Math.round(document.querySelector(\".sessions-dock-shell .dv-groupview\").getBoundingClientRect().width)")
            print("after sash drag w:", w1, "->", "SASH-PASS" if abs(w1-w0) > 100 else "SASH-FAIL")
        r = await cmd(ws, "Page.captureScreenshot", {"format": "png"})
        open("/tmp/oly-qa/30-split-fixed.png", "wb").write(base64.b64decode(r["data"]))
        print("shot saved")
asyncio.run(main())
