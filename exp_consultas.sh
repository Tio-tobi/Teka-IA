#!/usr/bin/env bash
# CONSULTAS 30->77 (25 de uma palavra so) + 12 moldes "sugere/recomenda".
#
# Instrumento: sonda_consulta.sh, dirigida. O benchmark de 150 tem desvio ~8 entre
# sementes e nao resolve efeito de 3 a 5 frases com n=3 — medido na sexta. Ele roda
# junto so para vigiar regressao grande, nao para decidir.
#
# Antes: ferramenta 8,67/12 | argumento 6,33/12 | com preposicao 1,33
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
for s in 7 8 9; do
    echo "=== consultas semente ${s} — $(date +%H:%M) ==="
    ./teka_exp.exe agente --epocas 12 --exemplos 16000 --semente "$s" --threads 10 \
        --saida "modelos/teka_cons_s${s}.bin" --benchmark > "cons_s${s}.log" 2>&1
    grep -aE 'ferramenta certa:|argumento certo' "cons_s${s}.log"
    bash sonda_consulta.sh "teka_cons_s${s}.bin"
done
