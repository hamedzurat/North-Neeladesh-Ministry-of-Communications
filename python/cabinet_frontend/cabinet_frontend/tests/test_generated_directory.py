from __future__ import annotations

import unittest

from cabinet_frontend.generated_directory import generated_directory_page, unknown_id


class GeneratedDirectoryTests(unittest.TestCase):
    def test_generation_is_deterministic(self) -> None:
        self.assertEqual(generated_directory_page(1), generated_directory_page(1))

    def test_generation_is_seeded_by_id(self) -> None:
        page = generated_directory_page(1)
        self.assertIsNotNone(page)
        assert page is not None
        self.assertEqual(page["directory_id"], 1)
        self.assertTrue(any(line.startswith("NAME // ") for line in page["lines"]))

    def test_some_ids_are_intentionally_empty(self) -> None:
        self.assertIsNone(generated_directory_page(0))
        self.assertIsNone(generated_directory_page(9999))

    def test_extracts_unknown_directory_id(self) -> None:
        self.assertEqual(unknown_id(["SUBSCRIBER ID 0042"]), 42)
        self.assertIsNone(unknown_id(["NO RECORD"]))


if __name__ == "__main__":
    unittest.main()
