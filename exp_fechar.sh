#!/usr/bin/env bash
# Fecha o benchmark: sementes 13 a 18, somadas as 7-12 ja rodadas.
#
# Doze sementes porque tres desmentiram tres vezes nesta semana (o tronco, a
# entropia, e os 116,3 de hoje). Com desvio ~4 entre sementes, doze resolvem efeito
# de ~5 frases; tres nao resolvem nem 15.
#
# O `n` esta FIXADO aqui e nao vai ser reinterpretado no meio: parcial nao e
# estimativa do final, e este projeto ja pagou por essa licao tres vezes.
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
for s in 13 14 15 16 17 18; do
    echo "=== semente ${s} — $(date +%H:%M) ==="
    ./teka_exp.exe agente --epocas 12 --exemplos 16000 --semente "$s" --threads 10 \
        --saida "modelos/teka_f_s${s}.bin" --benchmark > "f_s${s}.log" 2>&1
    grep -aE 'ferramenta certa:|argumento quando' "f_s${s}.log"
    bash sonda_consulta.sh "teka_f_s${s}.bin"
done
