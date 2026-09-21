from __future__ import annotations

import unittest

from panemorph.actions.open_selector import selector_command


class OpenSelectorTests(unittest.TestCase):
    def test_overlay_implicitly_targets_active_pane(self) -> None:
        command = selector_command("herdr", "dev.panemorph", "w1:p2", "send")

        self.assertNotIn("--target-pane", command)
        self.assertIn("PANEMORPH_MODE=send", command)
        self.assertIn("PANEMORPH_SOURCE_PANE_ID=w1:p2", command)
        self.assertEqual(command[-1], "--focus")


if __name__ == "__main__":
    unittest.main()
