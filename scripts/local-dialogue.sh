#!/usr/bin/env bash
set -euo pipefail

dialogue_url="http://127.0.0.1:${NN_MVP_LLM_PORT:?Set by just backend}/v1/chat/completions"
prompt=$(cat)
payload=$(jq --null-input --arg prompt "$prompt" '{messages: [{role: "user", content: $prompt}], temperature: 0.2, max_tokens: 70, stream: false}')
curl --fail-with-body --silent --show-error \
  --max-time 30 \
  --header 'Content-Type: application/json' \
  --data "$payload" \
  "$dialogue_url" | jq --raw-output '.choices[0].message.content'
