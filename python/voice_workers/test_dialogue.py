import unittest

from voice_workers.dialogue import NATURAL_REPLY_INSTRUCTIONS, prompt_for


class DialoguePromptTests(unittest.TestCase):
    def test_prompt_adds_natural_conversation_rules(self) -> None:
        prompt = prompt_for(
            {
                "context": {
                    "profile": {"name": "Nusrat Rahman", "role": "architect"},
                    "caller_place": "Shapla Apartments",
                    "requested_place": "Mohona Heights",
                },
                "transcript": "Operator: Do you have any pets?\nSubscriber:",
            }
        )

        self.assertIn(NATURAL_REPLY_INSTRUCTIONS, prompt)
        self.assertIn("Reply to the operator's latest question", prompt)
        self.assertIn(
            'never give only a generic acknowledgement such as "I will answer that"', prompt
        )
        self.assertIn("avoid asking for information the operator already gave you", prompt)
        self.assertIn("steer back to your immediate goal", prompt)

    def test_prompt_keeps_story_context_and_transcript(self) -> None:
        prompt = prompt_for(
            {
                "context": {
                    "profile": {"name": "Nusrat Rahman"},
                    "caller_place": "Shapla Apartments",
                },
                "transcript": "Operator: Please tell me your address.\nSubscriber:",
            }
        )

        self.assertIn('"name":"Nusrat Rahman"', prompt)
        self.assertIn("Shapla Apartments", prompt)
        self.assertIn("Please tell me your address.", prompt)

    def test_story_guidance_is_explicit_and_authoritative(self) -> None:
        prompt = prompt_for(
            {
                "context": {
                    "profile": {"name": "Nusrat Rahman"},
                    "call_guidance": (
                        'Say directly: "You failed to help, and I will pursue you for the loss."'
                    ),
                },
                "transcript": "How did things turn out for you?",
            }
        )

        self.assertIn("AUTHORITATIVE STORY GUIDANCE:", prompt)
        self.assertIn("You failed to help, and I will pursue you for the loss.", prompt)
        self.assertIn("LATEST OPERATOR UTTERANCE:", prompt)
        self.assertIn("Follow the story guidance for this turn exactly.", prompt)


if __name__ == "__main__":
    unittest.main()
