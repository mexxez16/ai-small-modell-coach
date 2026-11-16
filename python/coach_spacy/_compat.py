"""Forward-compatible extraction of spaCy training logger `info` dicts.

spaCy's logger protocol has changed the score shape across versions:
v3.8 provides `info["score"]` and `info["other_scores"]`, future versions may
consolidate to a single `info["scores"]`. Everything here reads keys
defensively so the logger keeps working across 3.8 -> 4.x.
"""

from __future__ import annotations

from typing import Any, Dict, List, Optional, Tuple

Number = float


def extract_score(info: Optional[Dict[str, Any]]) -> Tuple[Optional[Number], dict]:
    """Return (score, other_scores) from a loggers-protocol info dict.

    Handles:
      - v3.8 style: ``score`` scalar + ``other_scores`` dict
      - possible future style: ``scores`` dict only
      - nested dicts under other_scores are tables, not floats: skipped
    """
    if not info:
        return None, {}

    raw_other = info.get("other_scores", {})

    score = info.get("score")
    other_scores: Dict[str, Any] = (
        raw_other if isinstance(raw_other, dict) else {}
    )

    if score is None:
        candidate = info.get("scores", {})
        if isinstance(candidate, dict):
            # A single scalar under a "score" key, or a flat metrics dict.
            if "score" in candidate and isinstance(candidate["score"], (int, float)):
                score = candidate["score"]
                other_scores = {
                    k: v for k, v in candidate.items() if k != "score"
                }
            else:
                other_scores = candidate

    if not isinstance(score, (int, float)):
        score = None

    return score, other_scores


def flatten_metrics(
    info: Optional[Dict[str, Any]],
) -> List[Tuple[str, Number, Dict[str, Any]]]:
    """Extract flat (name, value, context) triples from an info dict.

    Includes ``losses`` and any scalar entries in other_scores; skips nested
    dicts (tables) and unknown non-numeric values.
    """
    if not info:
        return []
    out: List[Tuple[str, Number, Dict[str, Any]]] = []

    losses = info.get("losses") or {}
    if isinstance(losses, dict):
        for name, value in losses.items():
            if isinstance(value, (int, float)):
                out.append((str(name), float(value), {"type": "loss"}))

    _, other_scores = extract_score(info)
    for name, value in other_scores.items():
        if isinstance(value, (int, float)):
            out.append((str(name), float(value), {"type": "other_scores"}))

    return out


def extract_context(info: Optional[Dict[str, Any]]) -> Dict[str, Any]:
    """Scalar step context that is not a metric: epoch, seconds, words, ..."""
    if not info:
        return {}
    out: Dict[str, Any] = {}
    for key in ("epoch", "step", "seconds", "words", "checkpoints"):
        value = info.get(key)
        if isinstance(value, (int, float, str)):
            out[key] = value
    return out
