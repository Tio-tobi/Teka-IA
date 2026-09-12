#!/usr/bin/env bash
# Confirmacao dos 116,3. Sementes 10, 11, 12 — as mesmas que expuseram que a
# "linha de base de 112,7" era um lote com sorte.
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
for s in 10 11 12; do
    echo "=== conf semente ${s} — $(date +%H:%M) ==="
    ./teka_exp.exe agente --epocas 12 --exemplos 16000 --semente "$s" --threads 10 \
        --saida "modelos/teka_conf_s${s}.bin" --benchmark > "conf_s${s}.log" 2>&1
    grep -aE 'ferramenta certa:|argumento quando' "conf_s${s}.log"
    bash sonda_consulta.sh "teka_conf_s${s}.bin"
    bash sonda_amigo.sh "teka_conf_s${s}.bin" 2>&1 | tail -1
done
