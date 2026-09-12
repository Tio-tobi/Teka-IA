#!/usr/bin/env bash
# Os 54 moldes de forma INDIRETA valem alguma coisa?
#
# Diagnostico: 56 dos 239 erros eram abstencao, e quase todos em pedido que nao
# NOMEIA o recurso ("a maquina esta pesada" = memoria). Todo molde existente nomeia.
#
# Base a bater: braco de entropia, 6 sementes, media 110,2 (105,113,111,116,114,102).
# Entropia agora e padrao, entao nao ha flag de patcher aqui.
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
for s in 7 8 9; do
    echo "=== indiretos semente ${s} — $(date +%H:%M) ==="
    ./teka_exp.exe agente --epocas 12 --exemplos 16000 --semente "$s" --threads 10 \
        --saida "modelos/teka_ind_s${s}.bin" --benchmark > "ind_s${s}.log" 2>&1
    grep -aE 'ferramenta certa:|argumento certo' "ind_s${s}.log"
done
