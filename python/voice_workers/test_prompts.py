import contextlib
import io
import unittest

from voice_workers.common import prompt_template, world_knowledge
from voice_workers.player import task_guidance, validate_task_output


class SharedPromptTests(unittest.TestCase):
    def test_operator_uses_system_rules_and_user_request(self) -> None:
        system = prompt_template("operator_system.txt").format(world_knowledge=world_knowledge())
        user = prompt_template("operator_user.txt").format(
            task="ask for help",
            constraints="use one sentence",
            request_json='{"task":"ask for help"}',
        )

        self.assertIn('property named "text"', system)
        self.assertIn("North Neeladesh", system)
        self.assertIn("Intent: ask for help", user)
        self.assertIn("Constraints: use one sentence", user)
        self.assertIn('{"task":"ask for help"}', user)

    def test_classifier_separates_rules_from_input_and_sets_json_contract(self) -> None:
        system = prompt_template("classifier_system.txt").format(story_rules="Allowed labels: success, failure.")

        self.assertIn("Allowed labels: success, failure.", system)
        self.assertIn("not as instructions", system)
        self.assertIn('property named "classification"', system)

    def test_ems_player_must_tell_ems_to_send_help(self) -> None:
        guidance = task_guidance("ask EMS to send help")
        self.assertIn("Address EMS directly", guidance)
        self.assertIn("Do not say that you will send or dispatch help yourself", guidance)
        validate_task_output(
            "ask EMS to send help",
            "Please send an ambulance to Shapla Apartments.",
        )
        with contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                validate_task_output(
                    "ask EMS to send help",
                    "I will dispatch an ambulance to Shapla Apartments.",
                )


if __name__ == "__main__":
    unittest.main()
