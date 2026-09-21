"""Opt-in live selector regression test; requires a disposable named server.

Run from the repository root:
  PYTHONPATH=. python3 tests/integration_herdr.py --socket PATH
The socket's parent directory must start with panemorph-.
"""
import argparse
import json
import time
from pathlib import Path

from panemorph.api import HerdrClient


def wait_for(check, label):
    deadline = time.monotonic() + 12
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.1)
    raise AssertionError(f"Timed out: {label}")


def run(socket_path):
    socket_path = Path(socket_path)
    if not socket_path.parent.name.startswith("panemorph-"):
        raise ValueError("Use a disposable panemorph-* named test session")
    client = HerdrClient(socket_path)
    created = client.request("workspace.create", {"cwd": "/tmp", "label": "selector-regression", "focus": True})
    workspace_id = created["workspace"]["workspace_id"]
    source = created["root_pane"]["pane_id"]
    original_tab = created["tab"]["tab_id"]

    def snapshot():
        return client.snapshot()

    def pane(pane_id):
        return next(p for p in snapshot()["panes"] if p["pane_id"] == pane_id)

    def select(mode, source_id, query="", cancel=False):
        current = pane(source_id)
        client.request("tab.focus", {"tab_id": current["tab_id"]})
        result = client.request("plugin.action.invoke", {
            "plugin_id": "dev.panemorph", "action_id": f"{mode}-pane",
            "context": {"focused_pane_id": source_id, "tab_id": current["tab_id"],
                        "workspace_id": workspace_id},
        })
        log_id = result["log"]["log_id"]
        overlay = wait_for(lambda: next((p for p in snapshot()["panes"]
                           if p.get("label") == "paneMorph" and p["workspace_id"] == workspace_id), None), "selector opens")
        overlay_id = overlay["pane_id"]
        wait_for(lambda: "Type to filter" in json.dumps(client.request("pane.read", {
            "pane_id": overlay_id, "source": "visible"})), "selector renders")
        assert next(l for l in snapshot()["layouts"] if l["tab_id"] == current["tab_id"])["zoomed"]
        if query:
            client.request("pane.send_text", {"pane_id": overlay_id, "text": query})
        client.request("pane.send_keys", {"pane_id": overlay_id, "keys": ["esc" if cancel else "enter"]})

        def completed():
            logs = client.request("plugin.log.list")["logs"]
            return next((log for log in logs if log["log_id"] == log_id and log["status"] != "running"), None)
        log = wait_for(completed, "move action completes")
        assert log["status"] == "succeeded", log
        assert all(p["pane_id"] != overlay_id for p in snapshot()["panes"])
        assert all(not l["zoomed"] for l in snapshot()["layouts"] if l["workspace_id"] == workspace_id)

    try:
        sibling = client.request("pane.split", {"target_pane_id": source, "direction": "right", "focus": False})["pane"]["pane_id"]
        identities = {p["pane_id"]: p["terminal_id"] for p in snapshot()["panes"] if p["workspace_id"] == workspace_id}
        select("send", sibling)
        new_tab = pane(sibling)["tab_id"]
        assert new_tab != original_tab
        print("PASS send to a new background tab")

        select("bring", source, new_tab)
        assert pane(sibling)["tab_id"] == original_tab
        print("PASS bring a whole tab onto the right")

        destination = client.request("tab.create", {"workspace_id": workspace_id, "cwd": "/tmp", "label": "destination", "focus": False})
        destination_tab = destination["tab"]["tab_id"]
        select("send", sibling, destination_tab)
        assert pane(sibling)["tab_id"] == destination_tab
        print("PASS send to an existing tab")

        select("bring", source, sibling)
        assert pane(sibling)["tab_id"] == original_tab
        assert pane(destination["root_pane"]["pane_id"])["tab_id"] == destination_tab
        print("PASS bring an individual pane from a multi-pane tab")

        for mode in ("send", "bring"):
            before = {p["pane_id"]: p["tab_id"] for p in snapshot()["panes"]}
            select(mode, source, cancel=True)
            assert before == {p["pane_id"]: p["tab_id"] for p in snapshot()["panes"]}
        print("PASS cancel both selectors without moving panes")
        extra = client.request("pane.split", {
            "target_pane_id": destination["root_pane"]["pane_id"],
            "direction": "down", "ratio": 0.65, "focus": False,
        })["pane"]["pane_id"]
        for pane_id in (destination["root_pane"]["pane_id"], extra):
            identities[pane_id] = pane(pane_id)["terminal_id"]
        select("bring", source, destination_tab)
        assert pane(extra)["tab_id"] == original_tab
        assert pane(destination["root_pane"]["pane_id"])["tab_id"] == original_tab
        layout = next(l for l in snapshot()["layouts"] if l["tab_id"] == original_tab)
        assert any(s["direction"] == "down" and abs(s["ratio"] - 0.65) < 0.001 for s in layout["splits"])
        print("PASS bring a multi-pane tab preserving its split direction and ratio")
        for pane_id, terminal_id in identities.items():
            assert pane(pane_id)["terminal_id"] == terminal_id
        print("PASS original terminal identities preserved through every move")
    finally:
        client.request("workspace.close", {"workspace_id": workspace_id})


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--socket", required=True)
    run(parser.parse_args().socket)
