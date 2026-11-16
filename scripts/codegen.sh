#!/usr/bin/env bash
# Regenerate apps/web/src/api/schema-openapi.d.ts from the running coach-api.
# Usage: scripts/codegen.sh [base_url]   (default http://127.0.0.1:8000)
set -euo pipefail

BASE_URL="${1:-http://127.0.0.1:8000}"
OUT="apps/web/src/api/schema-openapi.d.ts"

curl -fsS "${BASE_URL}/api/openapi.json" -o /tmp/coach-openapi.json
cd apps/web
npx openapi-typescript /tmp/coach-openapi.json -o src/api/schema-openapi.d.ts
echo "regenerated ${OUT%.*}.d.ts (relative to repo root: ${OUT})"
