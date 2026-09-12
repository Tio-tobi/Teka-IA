#!/usr/bin/env bash
# Frases longas no treino + janela 64->128.
#
# Antes (modelos cons_*): teste do amigo ferramenta 9/10, argumento 0/10.
# A causa: mediana de treino 37 bytes, as frases dele tem 106. Ela nunca viu aquilo.
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
for s in 7 8 9; do
    echo "=== longas semente ${s} — $(date +%H:%M) ==="
    ./teka_exp.exe agente --epocas 12 --exemplos 16000 --semente "$s" --threads 10 \
        --saida "modelos/teka_long_s${s}.bin" --benchmark > "long_s${s}.log" 2>&1
    grep -aE 'ferramenta certa:|argumento quando' "long_s${s}.log"
    bash sonda_consulta.sh "teka_long_s${s}.bin"
    bash sonda_amigo.sh "teka_long_s${s}.bin" 2>&1 | tail -1
done
