#!/usr/bin/env bash
# Sessão longa e desacompanhada. Roda sozinha até o fim e deixa relatório.
#
# 12 threads e batch 48.
#
# O padrão era batch 8, e com ele o processo usava 4,2 das 12 threads. A causa não
# é banda de memória: o RG-LRU tem um laço TEMPORAL sequencial (h_t depende de
# h_{t-1}), então o paralelismo é sobre batch x dimensão. Com batch 8 não há
# trabalho para espalhar.
#
# Medido, 12 threads, preset padrao:
#   batch   8 -> 2.974 bytes/s      batch  48 -> 4.056 bytes/s   (+36%)
#   batch  24 -> 3.760 bytes/s      batch  96 -> usa 5,1 GB      (swap)
#
# 48 é o ponto: ganha 36% e cabe nos 2,5 GB livres com o MIR4 aberto.
#
# O que ela testa, e por que vale as horas:
#
#   O tronco pré-treinado já foi testado hoje e transferiu 0,3 frase — mas com um
#   LM de 1,6M treinado por 30 min. Isto testa o mesmo em outro regime: 12M de
#   parâmetros e 4 horas, sobre o corpus técnico que derrubou bits/byte de 5,52
#   para 1,98.
#
#   Não é aposta alta. É a única configuração que horas de máquina compram, e o
#   resultado é conclusivo dos dois jeitos: se transferir, abre um caminho; se não
#   transferir, encerra a questão do tronco de vez.
set -u
cd "$(dirname "$0")"
T=12
LOG=sessao_noturna.log

diga() { echo "[$(date +%H:%M:%S)] $*" | tee -a "$LOG"; }

diga "=== inicio ==="
diga "fase 1/4: LM padrao (12M) sobre corpus_misto, 240 min, $T threads"
./target/release/teka.exe treinar --preset padrao --corpus dados/corpus_misto.txt \
  --minutos 240 --threads $T --batch 48 --saida lm_grande.bin >> lm_grande.log 2>&1
diga "fase 1 terminou"

diga "fase 2/4: bits/byte nos dois dominios"
for m in lm_misto lm_grande; do
  for t in val_tec val_lit; do
    v=$(./target/release/teka.exe medir-lm --carregar "$m.bin" --texto "dados/$t.txt" \
        --threads $T 2>/dev/null | grep -oE '[0-9]+\.[0-9]{4}')
    diga "  $m / $t : ${v:-falhou} bits/byte"
  done
done

diga "fase 3/4: agente sobre o tronco novo, 3 sementes"
for s in 7 8 9; do
  ./target/release/teka.exe agente --epocas 12 --exemplos 16000 --semente $s \
    --cerebro lm_grande.bin --threads $T \
    --saida "teka_grande_s$s.bin" --benchmark > "grande_s$s.log" 2>&1
  r=$(grep -E "ferramenta certa:" "grande_s$s.log" | tail -1)
  diga "  semente $s: $r"
done

diga "fase 4/4: comparacao com a base"
for s in 7 8 9; do
  b=$(grep -E "ferramenta certa:" "bal_s$s.log" 2>/dev/null | tail -1)
  diga "  base    s$s: ${b:-sem log}"
done

diga "=== fim ==="
echo SESSAO_PRONTA >> "$LOG"
