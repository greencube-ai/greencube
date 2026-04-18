"""Prompt-injection probe. NOT a greencube benchmark.

This package tests a single hypothesis: does adding a hand-written
"previously on a similar task you made mistake X, don't do X" correction
to the system prompt measurably reduce how often the LLM repeats that
mistake class on a related task?

It does not exercise greencube. It does not test greencube's
correction-generation (that does not exist in Python here). The
correction strings are hand-written best-case oracles.
"""
