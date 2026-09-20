from __future__ import annotations

import unittest

from panemorph.topology import leaf_ids, plan_tab_merge


def pane(pane_id: str) -> dict:
    return {"type": "pane", "pane_id": pane_id}


def split(direction: str, ratio: float, first: dict, second: dict) -> dict:
    return {
        "type": "split",
        "direction": direction,
        "ratio": ratio,
        "first": first,
        "second": second,
    }


class TopologyPlannerTests(unittest.TestCase):
    def test_single_pane_tab_becomes_one_right_move(self) -> None:
        steps = plan_tab_merge(pane("w1:p1"), "w1:t2", "w1:p9")
        self.assertEqual(len(steps), 1)
        self.assertEqual(steps[0].pane_id, "w1:p1")
        self.assertEqual(steps[0].target_pane_id, "w1:p9")
        self.assertEqual(steps[0].split, "right")

    def test_nested_tree_preserves_source_directions_and_ratios(self) -> None:
        root = split(
            "down",
            0.6,
            split("right", 0.4, pane("w1:p1"), pane("w1:p2")),
            pane("w1:p3"),
        )
        steps = plan_tab_merge(root, "w1:t9", "w1:p8", outer_ratio=0.55)
        self.assertEqual([step.pane_id for step in steps], ["w1:p1", "w1:p3", "w1:p2"])
        self.assertEqual([step.target_pane_id for step in steps], ["w1:p8", "w1:p1", "w1:p1"])
        self.assertEqual([step.split for step in steps], ["right", "down", "right"])
        self.assertEqual([step.ratio for step in steps], [0.55, 0.6, 0.4])
        self.assertEqual(list(leaf_ids(root)), ["w1:p1", "w1:p2", "w1:p3"])

    def test_move_params_match_socket_schema(self) -> None:
        step = plan_tab_merge(pane("w1:p1"), "w1:t2", "w1:p9")[0]
        self.assertEqual(
            step.params(),
            {
                "pane_id": "w1:p1",
                "destination": {
                    "type": "tab",
                    "tab_id": "w1:t2",
                    "target_pane_id": "w1:p9",
                    "split": "right",
                    "ratio": 0.5,
                },
                "focus": False,
            },
        )


if __name__ == "__main__":
    unittest.main()
