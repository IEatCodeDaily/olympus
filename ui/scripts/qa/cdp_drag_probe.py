import asyncio, base64, json, sys, urllib.request, glob
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
    return r["result"].get("value")
async def mouse(ws, t, x, y, button="left", buttons=1):
    await cmd(ws, "Input.dispatchMouseEvent", {"type": t, "x": x, "y": y, "button": button, "buttons": buttons, "clickCount": 1 if t in ("mousePressed","mouseReleased") else 0})
async def drag(ws, x, y0, y1):
    await mouse(ws, "mousePressed", x, y0)
    for i in range(1, 11):
        await mouse(ws, "mouseMoved", x, y0 + (y1-y0)*i//10)
        await asyncio.sleep(0.02)
    await mouse(ws, "mouseReleased", x, y1, buttons=0)
async def main():
    tabs = json.load(urllib.request.urlopen(f"{CDP}/json/list"))
    page = next(t for t in tabs if t["type"] == "page")
    async with websockets.connect(page["webSocketDebuggerUrl"], max_size=50_000_000) as ws:
        await cmd(ws, "Page.enable"); await cmd(ws, "Runtime.enable")
        g = await ev(ws, "(() => { const b = document.querySelector(\".chatcol .rz-y\"); if (!b) return null; const r = b.getBoundingClientRect(); return {x: Math.round(r.x + r.width/2), y: Math.round(r.y + r.height/2)} })()")
        h0 = await ev(ws, "document.querySelector(\".bpanel\")?.getBoundingClientRect().height")
        print("bar:", g, "h0:", h0)
        if not g: return
        await drag(ws, g["x"], g["y"], g["y"] - 120)   # drag up: grow panel
        await asyncio.sleep(0.5)
        h1 = await ev(ws, "document.querySelector(\".bpanel\")?.getBoundingClientRect().height")
        g2 = await ev(ws, "(() => { const r = document.querySelector(\".chatcol .rz-y\").getBoundingClientRect(); return {x: Math.round(r.x + r.width/2), y: Math.round(r.y + r.height/2)} })()")
        await drag(ws, g2["x"], g2["y"], g2["y"] - 60) # second consecutive drag (regression: chaining)
        await asyncio.sleep(0.5)
        h2 = await ev(ws, "document.querySelector(\".bpanel\")?.getBoundingClientRect().height")
        print(f"h0={h0} h1={h1} h2={h2}")
        print("PASS" if h1 > h0 + 100 and h2 > h1 + 40 else "FAIL")
asyncio.run(main())
