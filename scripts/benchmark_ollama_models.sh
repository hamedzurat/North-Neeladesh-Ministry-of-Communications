#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
stamp="$(date +%Y%m%d-%H%M%S)"
out="${root}/benchmarks/ollama-${stamp}"
mkdir -p "${out}"

profiles=(
    "qwen35-4b-no-thinking|qwen3.5:4b|false|96|80|8"
    "qwen35-0.8b-no-thinking|qwen3.5:0.8b|false|96|80|8"
    "qwen35-2b-no-thinking|qwen3.5:2b|false|96|80|8"
)
paths=(ems_success ems_failure police_success water_no_help unrelated_questions random_conversation neel_direct neel_misdirection neel_professor_questions neel_tap neel_tap_reverse neel_tap_late neel_rewire neel_rewire_late neel_patience neel_arnab_patience neel_bela_1031 neel_bela_1032 neel_bela_1032_questions neel_arnab_unrelated_questions dirty_good dirty_neutral dirty_bad nahid_police_success nahid_five_scams cross_thread_nahid)

backend_pid=""
cleanup() {
    if [[ -n "${backend_pid}" ]] && kill -0 "${backend_pid}" 2>/dev/null; then
        kill "${backend_pid}" 2>/dev/null || true
        wait "${backend_pid}" 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM

printf 'profile\tmodel\tthink\twall_seconds\tstatus\n' > "${out}/summary.tsv"
for profile in "${profiles[@]}"; do
    IFS='|' read -r name model think dialogue_budget player_budget classifier_budget <<< "${profile}"
    profile_dir="${out}/${name}"
    mkdir -p "${profile_dir}"

    NN_OLLAMA_MODEL="${model}" \
    NN_OLLAMA_CLASSIFIER_MODEL="${model}" \
    NN_OLLAMA_PLAYER_MODEL="${model}" \
    NN_OLLAMA_THINK="${think}" \
    NN_OLLAMA_DIALOGUE_NUM_PREDICT="${dialogue_budget}" \
    NN_OLLAMA_PLAYER_NUM_PREDICT="${player_budget}" \
    NN_OLLAMA_CLASSIFIER_NUM_PREDICT="${classifier_budget}" \
    just --justfile "${root}/Justfile" story-test-backend > "${profile_dir}/backend.log" 2>&1 &
    backend_pid=$!

    ready=false
    for _ in $(seq 1 120); do
        if (echo >/dev/tcp/127.0.0.1/7878) 2>/dev/null && \
           (echo >/dev/tcp/127.0.0.1/7880) 2>/dev/null && \
           (echo >/dev/tcp/127.0.0.1/7882) 2>/dev/null; then
            ready=true
            break
        fi
        sleep 0.25
    done
    if [[ "${ready}" != true ]]; then
        echo "backend did not become ready for ${name}" >&2
        exit 1
    fi

    start=$SECONDS
    status=pass
    : > "${profile_dir}/transcript.log"
    printf 'path\tstatus\n' > "${profile_dir}/paths.tsv"
    : > "${profile_dir}/runner.log"
    for path in "${paths[@]}"; do
        temp="${profile_dir}/${path}.log"
        path_status=pass
        if ! NN_OLLAMA_MODEL="${model}" \
             NN_OLLAMA_CLASSIFIER_MODEL="${model}" \
             NN_OLLAMA_PLAYER_MODEL="${model}" \
             NN_OLLAMA_THINK="${think}" \
             NN_OLLAMA_DIALOGUE_NUM_PREDICT="${dialogue_budget}" \
             NN_OLLAMA_PLAYER_NUM_PREDICT="${player_budget}" \
             NN_OLLAMA_CLASSIFIER_NUM_PREDICT="${classifier_budget}" \
             just --justfile "${root}/Justfile" story-test "${path}" "${temp}" \
             >> "${profile_dir}/runner.log" 2>&1; then
            path_status=fail
            status=fail
        fi
        printf '%s\t%s\n' "${path}" "${path_status}" >> "${profile_dir}/paths.tsv"
        if [[ -f "${temp}" ]]; then
            cat "${temp}" >> "${profile_dir}/transcript.log"
            printf '\n' >> "${profile_dir}/transcript.log"
        fi
    done
    elapsed=$((SECONDS - start))
    printf '%s\t%s\t%s\t%s\t%s\n' "${name}" "${model}" "${think}" "${elapsed}" "${status}" >> "${out}/summary.tsv"

    kill "${backend_pid}" 2>/dev/null || true
    wait "${backend_pid}" 2>/dev/null || true
    backend_pid=""
done

cat > "${out}/README.md" <<EOF
# Ollama model comparison

Generated: ${stamp}

Each profile ran every story-test path with a fresh backend. The complete test
transcript is in each profile's transcript.log; per-path results are in
paths.tsv; startup and runner diagnostics are in backend.log and runner.log.
summary.tsv records
wall time and whether the suite completed successfully.

The model is selected consistently for dialogue, classifier, and simulated
player workers. NN_OLLAMA_THINK controls Ollama's thinking mode.
EOF

printf 'Benchmark written to %s\n' "${out}"
