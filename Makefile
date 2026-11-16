# AI Small Modell Coach — developer tasks.
# CI is the source of truth for what "green" means; these targets mirror it.

PY ?= python3.13
VENV := $(shell pwd)/.venv
VENV_PY := $(VENV)/bin/python
PIP := $(VENV_PY) -m pip

.PHONY: help dev-setup build test serve wheel web-dev web-build web-lint release clean

release: build wheel web-build
	@echo "Release artifacts: target/wheels/*.whl, apps/web/dist"

help:
	@echo "make dev-setup   - bootstrap .venv (py3.13, spaCy) + npm install"
	@echo "make build       - cargo build --workspace"
	@echo "make test        - cargo test, python smoke tests, web typecheck"
	@echo "make serve       - run coach-api (serves apps/web/dist if built)"
	@echo "make wheel       - build the abi3-py313 SDK wheel with maturin"
	@echo "make web-dev     - vite dev server (proxies API to :8000)"
	@echo "make web-build   - production frontend build into apps/web/dist"
	@echo "make clean       - remove build artifacts"

$(VENV)/bin/activate:
	$(PY) -m venv --system-site-packages $(VENV)
	$(PIP) install -q -U pip
	$(PIP) install -q fastapi uvicorn httpx pytest maturin
	$(VENV_PY) -m spacy download en_core_web_sm

dev-setup: $(VENV)/bin/activate
	$(PIP) install -q -e python
	cd apps/web && npm install --no-audit --no-fund

build:
	cargo build --workspace

test: build
	cargo test --workspace
	$(VENV_PY) -m pytest python/tests -m ""
	cd apps/web && npx tsc --noEmit

serve: build
	@if [ -d apps/web/dist ]; then \
		COACH_WEB_DIST="$$(pwd)/apps/web/dist" cargo run -p coach-api; \
	else \
		echo "Hinweis: apps/web/dist fehlt — nur API, keine UI (make web-build)"; \
		cargo run -p coach-api; \
	fi

wheel: $(VENV)/bin/activate
	cd crates/coach-pyo3 && $(VENV)/bin/maturin build --find-interpreter

web-dev:
	cd apps/web && npm run dev

web-build:
	cd apps/web && npm run build

web-lint:
	cd apps/web && npm run lint

clean:
	cargo clean
	rm -rf apps/web/dist apps/web/node_modules $(VENV)
