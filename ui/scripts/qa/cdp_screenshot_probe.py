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
async def shot(ws, path):
    r = await cmd(ws, "Page.captureScreenshot", {"format": "png"})
    open(path, "wb").write(base64.b64decode(r["data"])); print("shot:", path)
async def main():
    tabs = json.load(urllib.request.urlopen(f"{CDP}/json/list"))
    page = next(t for t in tabs if t["type"] == "page")
    async with websockets.connect(page["webSocketDebuggerUrl"], max_size=50_000_000) as ws:
        await cmd(ws, "Page.enable"); await cmd(ws, "Runtime.enable")
        await shot(ws, "/tmp/oly-qa/10-shell.png")
        # open first session row
        print("open row:", await ev(ws, "(() => { const r = document.querySelector(\".srow .srow-main\") || document.querySelector(\".srow\"); if (r) { r.click(); return true } return false })()"))
        await asyncio.sleep(4)
        await shot(ws, "/tmp/oly-qa/11-session.png")
        # sidebar diag: navitems + srow classes
        print("srow classes:", await ev(ws, "[...document.querySelectorAll(\".srow\")].map(r => r.className)"))
        # bottom panel presence
        print("bpanel:", await ev(ws, "!!document.querySelector(\".bpanel\")"), "rz-y:", await ev(ws, "!!document.querySelector(\".rz-y\")"))
asyncio.run(main())
