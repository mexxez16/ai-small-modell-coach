"""Minimal HTTP client for coach-api. Standard library only."""

from __future__ import annotations

import json
import urllib.error
import urllib.request
from typing import Any, Dict, List, Optional


class CoachApiClientError(RuntimeError):
    pass


class CoachApiClient:
    """Thin client for the coach-api REST surface used by the logger and the
    render worker. Intentionally dependency-free (urllib) so the spaCy
    environment stays lean."""

    def __init__(self, base_url: str, token: Optional[str] = None, timeout: float = 10.0):
        self.base_url = base_url.rstrip("/")
        self.token = token
        self.timeout = timeout

    def _request(self, method: str, path: str, body: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
        url = f"{self.base_url}{path}"
        data = json.dumps(body).encode() if body is not None else None
        req = urllib.request.Request(url, data=data, method=method)
        req.add_header("Content-Type", "application/json")
        if self.token:
            req.add_header("Authorization", f"Bearer {self.token}")
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                payload = resp.read()
            return json.loads(payload) if payload else {}
        except urllib.error.HTTPError as err:
            detail = err.read().decode(errors="replace")
            raise CoachApiClientError(
                f"coach-api {method} {path} -> HTTP {err.code}: {detail}"
            ) from err
        except urllib.error.URLError as err:
            raise CoachApiClientError(
                f"coach-api {method} {path} unreachable: {err.reason}"
            ) from err

    def health(self) -> Dict[str, Any]:
        return self._request("GET", "/api/health")

    def create_run(
        self,
        name: str,
        session_id: Optional[str] = None,
        config: Optional[Dict[str, Any]] = None,
        hparams: Optional[Dict[str, Any]] = None,
        tags: Optional[List[str]] = None,
    ) -> Dict[str, Any]:
        body: Dict[str, Any] = {"name": name}
        if session_id:
            body["session_id"] = session_id
        if config is not None:
            body["config"] = config
        body["hparams"] = hparams or {}
        if tags:
            body["tags"] = tags
        return self._request("POST", "/api/runs", body)

    def append_metrics(self, run_id: str, points: List[Dict[str, Any]]) -> Dict[str, Any]:
        return self._request("POST", f"/api/runs/{run_id}/metrics", points)

    def put_image(
        self,
        run_id: str,
        name: str,
        png_base64: str,
        caption: str = "",
        step: Optional[int] = None,
        epoch: Optional[int] = None,
        meta: Optional[Dict[str, Any]] = None,
    ) -> Dict[str, Any]:
        body = {
            "name": name,
            "png_base64": png_base64,
            "caption": caption,
            "meta": meta or {},
        }
        if step is not None:
            body["step"] = step
        if epoch is not None:
            body["epoch"] = epoch
        return self._request("POST", f"/api/runs/{run_id}/images", body)

    def mark_run_status(self, run_id: str, status: str) -> Dict[str, Any]:
        return self._request("POST", f"/api/runs/{run_id}/status", {"status": status})
