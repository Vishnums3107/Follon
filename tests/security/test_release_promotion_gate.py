import unittest

from tools.release_promotion_gate import PromotionError, validate_approval


class ReleasePromotionGateTests(unittest.TestCase):
    def test_only_ordered_transitions_with_distinct_approver_are_allowed(self) -> None:
        validate_approval("development", "staging", "user.release", "user.approver", "change.1")
        validate_approval("staging", "production", "user.release", "user.approver", "change.2")
        with self.assertRaises(PromotionError):
            validate_approval("development", "production", "user.release", "user.approver", "change.3")
        with self.assertRaises(PromotionError):
            validate_approval("staging", "production", "user.same", "user.same", "change.4")


if __name__ == "__main__":
    unittest.main()
