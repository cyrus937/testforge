"""Compatibility exports for test post-processing.

This module preserves the public import path
`testforge_ai.generator.post_process` while the implementation lives in
`testforge_ai.generator.prompts.post_process`.
"""

from testforge_ai.generator.prompts.post_process import PostProcessor, ProcessedTest

__all__ = ["PostProcessor", "ProcessedTest"]
