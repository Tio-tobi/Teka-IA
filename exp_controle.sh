#!/usr/bin/env bash
# A pergunta que ficou sem controle: o patch por entropia compra alguma coisa?
#
# Ele custa +45% de backbone na distribuicao REAL de treino (3,343 contra 4,833
# bytes/patch sobre 16.000 exemplos) — nao os +24% que eu medira em linhas de corpus.
# E todos os doze treinos da semana usaram entropia. Nao ha controle.
#
# O -0,17 que eu citava como "encerra a linha" foi medido com dados VELHOS e janela
# 64. Nao vale mais para a configuracao de hoje.
#
# Braco de entropia ja existe: sementes 7 a 18, media 114,58, sd 3,55.
# Este script roda o controle `por_palavra` nas MESMAS doze sementes, pareado.
#
# n FIXADO em 12. Nao vou ler parcial.
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
for s in 7 8 9 10 11 12 13 14 15 16 17 18; do
    echo "=== por_palavra semente ${s} — $(date +%H:%M) ==="
    ./teka_exp.exe agente --patcher por_palavra --epocas 12 --exemplos 16000 \
        --semente "$s" --threads 10 --saida "modelos/teka_pp_s${s}.bin" --benchmark \
        > "logs/pp_s${s}.log" 2>&1
    grep -aE 'ferramenta certa:|argumento quando' "logs/pp_s${s}.log"
done
