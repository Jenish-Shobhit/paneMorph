from __future__ import annotations

import unittest

from panemorph.api import HerdrError
from panemorph.model import Choice
from panemorph.service import PaneMorphService


class FakeClient:
    def __init__(self, snapshot: dict, exports: dict[str, dict] | None = None) -> None:
        self._snapshot = snapshot
        self.exports = exports or {}
        self.requests: list[tuple[str, dict]] = []

    def snapshot(self):
        return self._snapshot

    def current_pane(self, pane_id):
        return next(pane for pane in self._snapshot["panes"] if pane["pane_id"] == pane_id)

    def request(self, method, params=None):
        params = params or {}
        self.requests.append((method, params))
        if method == "layout.export":
            return {"type": "layout_export", "layout": self.exports[params["tab_id"]]}
        if method == "pane.move":
            return {"type": "pane_move", "move_result": {"changed": True}}
        raise AssertionError(method)


def fixture() -> dict:
    return {
        "panes": [
            {"pane_id": "w1:p1", "tab_id": "w1:t1", "workspace_id": "w1", "terminal_title_stripped": "Editor"},
            {"pane_id": "w1:p2", "tab_id": "w1:t2", "workspace_id": "w1", "terminal_title_stripped": "Server"},
            {"pane_id": "w1:p3", "tab_id": "w1:t2", "workspace_id": "w1", "terminal_title_stripped": "Tests"},
        ],
        "tabs": [
            {"tab_id": "w1:t1", "workspace_id": "w1", "number": 1, "label": "Code", "pane_count": 1},
            {"tab_id": "w1:t2", "workspace_id": "w1", "number": 2, "label": "Runtime", "pane_count": 2},
        ],
        "layouts": [
            {"tab_id": "w1:t1", "workspace_id": "w1", "focused_pane_id": "w1:p1", "zoomed": False, "panes": [{"pane_id": "w1:p1"}]},
            {"tab_id": "w1:t2", "workspace_id": "w1", "focused_pane_id": "w1:p3", "zoomed": False, "panes": [{"pane_id": "w1:p2"}, {"pane_id": "w1:p3"}]},
        ],
    }


class ServiceTests(unittest.TestCase):
    def test_send_choices_include_new_tab_and_other_tabs(self) -> None:
        snapshot = fixture()
        service = PaneMorphService(FakeClient(snapshot))
        choices = service.send_choices(snapshot["panes"][0], snapshot)
        self.assertEqual([choice.kind for choice in choices], ["new_tab", "tab"])
        self.assertEqual(choices[1].tab_id, "w1:t2")

    def test_bring_choices_group_tab_and_child_panes(self) -> None:
        snapshot = fixture()
        service = PaneMorphService(FakeClient(snapshot))
        choices = service.bring_choices(snapshot["panes"][0], snapshot)
        self.assertEqual([choice.kind for choice in choices], ["tab", "pane", "pane"])
        self.assertEqual([choice.depth for choice in choices], [0, 1, 1])

    def test_send_to_existing_tab_uses_its_focused_pane(self) -> None:
        snapshot = fixture()
        client = FakeClient(snapshot)
        service = PaneMorphService(client)
        service.send(snapshot["panes"][0], Choice("tab", "Runtime", tab_id="w1:t2"))
        move = client.requests[-1][1]
        self.assertEqual(move["destination"]["target_pane_id"], "w1:p3")
        self.assertEqual(move["destination"]["split"], "right")

    def test_bring_whole_tab_executes_live_tree_plan(self) -> None:
        snapshot = fixture()
        export = {
            "w1:t2": {
                "tab_id": "w1:t2",
                "root": {
                    "type": "split",
                    "direction": "down",
                    "ratio": 0.6,
                    "first": {"type": "pane", "pane_id": "w1:p2"},
                    "second": {"type": "pane", "pane_id": "w1:p3"},
                },
            }
        }
        client = FakeClient(snapshot, export)
        service = PaneMorphService(client)
        service.bring(snapshot["panes"][0], Choice("tab", "Runtime", tab_id="w1:t2"))
        moves = [params for method, params in client.requests if method == "pane.move"]
        self.assertEqual([move["pane_id"] for move in moves], ["w1:p2", "w1:p3"])
        self.assertEqual([move["destination"]["split"] for move in moves], ["right", "down"])

    def test_zoomed_destination_is_rejected_before_move(self) -> None:
        snapshot = fixture()
        snapshot["layouts"][0]["zoomed"] = True
        client = FakeClient(snapshot)
        service = PaneMorphService(client)
        with self.assertRaisesRegex(HerdrError, "Unzoom"):
            service.send(snapshot["panes"][0], Choice("new_tab", "New"))
        self.assertEqual(client.requests, [])


if __name__ == "__main__":
    unittest.main()
