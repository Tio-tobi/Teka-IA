#!/usr/bin/env bash
# EXPERIMENTO B: os 66 fora-de-escopo valem alguma coisa?
#
# Diagnostico sobre 11 sementes: 14 frases de 150 falham em 9 ou mais. Cinco sao o
# mesmo modo — ela DEVERIA abster e age, agarrando qualquer ferramenta que
# compartilhe uma palavra ("queria te dar um abraco" -> procurar_arquivo).
#
# `perguntar` tinha 23 frases para cobrir o complemento de 18 ferramentas; o `hora`
# sozinho tem 42 para uma coisa so. Foram para 66.
#
# LINHA DE BASE: por_palavra, 12 sementes, 110,67 +- 4,48.
# Mesmo patcher, mesmas sementes, so os dados mudando. n FIXADO em 12.
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
for s in 7 8 9 10 11 12 13 14 15 16 17 18; do
    echo "=== fora66 semente ${s} — $(date +%H:%M) ==="
    ./teka_exp.exe agente --patcher por_palavra --epocas 12 --exemplos 16000 \
        --semente "$s" --threads 10 --saida "teka_f66_s${s}.bin" --benchmark \
        > "f66_s${s}.log" 2>&1
    grep -aE 'ferramenta certa:|argumento quando' "f66_s${s}.log"
done
