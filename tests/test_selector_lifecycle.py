import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

from panemorph.actions.open_selector import finish_selection
from panemorph.api import HerdrError


class SelectorLifecycleTests(unittest.TestCase):
    def test_move_waits_for_overlay_removal_and_refreshes_source(self):
        client = Mock()
        client.snapshot.side_effect = [
            {"panes": [{"pane_id": "overlay"}], "layouts": [{"zoomed": True}]},
            {"panes": [{"pane_id": "source"}], "layouts": [{"zoomed": False}]},
        ]
        with tempfile.TemporaryDirectory() as directory:
            result = Path(directory) / "result.json"
            result.write_text(json.dumps({"choice": {"kind": "new_tab", "title": "New"}}))
            with patch("panemorph.actions.open_selector.PaneMorphService") as service, \
                 patch("panemorph.actions.open_selector.time.sleep") as sleep:
                finish_selection(client, "source", "overlay", result, "send")
                sleep.assert_called_once()
                service.return_value.current.assert_called_once_with("source")
                service.return_value.send.assert_called_once()
                service.return_value.bring.assert_not_called()
                self.assertEqual(client.snapshot.call_count, 2)

    def test_cancel_never_moves_a_pane(self):
        client = Mock()
        client.snapshot.return_value = {"panes": []}
        with tempfile.TemporaryDirectory() as directory:
            result = Path(directory) / "result.json"
            result.write_text('{"choice": null}')
            with patch("panemorph.actions.open_selector.PaneMorphService") as service:
                finish_selection(client, "source", "overlay", result, "bring")
                service.assert_not_called()

    def test_selector_error_reaches_action_log(self):
        client = Mock()
        client.snapshot.return_value = {"panes": []}
        with tempfile.TemporaryDirectory() as directory:
            result = Path(directory) / "result.json"
            result.write_text('{"error": "Unable to draw selector"}')
            with self.assertRaisesRegex(HerdrError, "Unable to draw selector"):
                finish_selection(client, "source", "overlay", result, "send")
