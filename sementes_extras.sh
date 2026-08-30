#!/usr/bin/env bash
# Sementes extras + a CELULA QUE FALTAVA.
#
# O par medido ate 26/08 compara coisas de tamanhos diferentes:
#   base    = 1,6M do zero    -> 108,7/150
#   grande  = 12M com tronco  -> 111,3/150
# O +2,7 frase pode ser do pre-treino OU dos 7,5x parametros. Sem 12M-do-zero nao
# da para saber qual, e mais semente do par confundido so mede melhor um numero
# que nao responde a pergunta. Por isso o controle vem primeiro.
#
# Threads: os tres gemm de src/backend/parallel.rs fatiam por LINHA de saida, sem
# soma entre threads, e ha teste de igualdade exata contra o oraculo escalar. Logo
# 3x4 threads da o MESMO resultado que 1x12 — muda so o relogio. As sementes novas
# continuam comparaveis com as antigas.
#
# Mora na raiz do projeto de proposito: a versao anterior vivia em /tmp e fazia
# `cd $(dirname $0)`, o que a mandou esperar um batch.log que nunca existiria ali.
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
LOG=sementes_extras.log
T=4

diga() { echo "[$(date +%H:%M:%S)] $*" | tee -a "$LOG"; }

diga "=== inicio (pwd=$(pwd)) ==="

# $1 = rotulo, resto = sementes. Roda todas de uma vez, T threads cada.
onda() {
  local rot="$1"; shift
  local pids=() s extra
  for s in "$@"; do
    case "$rot" in
      scratch12) extra="--preset padrao" ;;
      grande)    extra="--cerebro lm_grande.bin" ;;
      base)      extra="--preset pequeno" ;;
      *)         diga "rotulo desconhecido: $rot"; return 1 ;;
    esac
    ./target/release/teka.exe agente --epocas 12 --exemplos 16000 --semente "$s" \
      $extra --threads $T --saida "teka_${rot}_s$s.bin" --benchmark \
      > "${rot}_s$s.log" 2>&1 &
    pids+=($!)
  done
  diga "onda $rot [$*] rodando, 4 threads cada"
  local p
  for p in "${pids[@]}"; do wait "$p"; done
  for s in "$@"; do
    diga "  $rot s$s: $(grep -E 'ferramenta certa:' "${rot}_s$s.log" | tail -1 || echo FALHOU)"
  done
  # 48 MB por .bin de 12M; so o numero do benchmark interessa.
  rm -f teka_scratch12_s*.bin
}

diga "--- 1/4: CONTROLE 12M do zero, sementes 7 8 9 (a celula que faltava) ---"
onda scratch12 7 8 9
diga "--- 2/4: 12M com tronco, sementes 10 11 12 ---"
onda grande 10 11 12
diga "--- 3/4: CONTROLE 12M do zero, sementes 10 11 12 ---"
onda scratch12 10 11 12
diga "--- 4/4: base 1,6M, sementes 10 11 12 ---"
onda base 10 11 12

diga "=== fim ==="
echo SEMENTES_PRONTAS >> "$LOG"
