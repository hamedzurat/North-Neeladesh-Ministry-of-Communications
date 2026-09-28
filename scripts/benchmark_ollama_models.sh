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
paths=(fallen_mother_ems_report_succeeds fallen_mother_ems_report_fails fallen_mother_police_report_succeeds fallen_mother_no_water_help_leads_to_bad_ending fallen_mother_unrelated_questions fallen_mother_random_conversation bela_bose_completes_professor_routing bela_bose_misdirection_reaches_bad_ending bela_bose_answers_professor_questions bela_bose_tap_monitors_call bela_bose_reverse_tap_wiring_monitors_call bela_bose_late_tap_monitors_call bela_bose_rewires_tap_monitoring bela_bose_patience_expires bela_bose_arnab_patience_expires bela_bose_wrong_bela_destination bela_bose_correct_bela_destination bela_bose_answers_bela_questions bela_bose_handles_arnab_unrelated_questions dirty_work_good_ending dirty_work_neutral_ending dirty_work_bad_ending nahid_police_report_stops_scams nahid_five_completed_scams_end_in_penalty nahid_dialogue_then_police_report_stops_scams)

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
