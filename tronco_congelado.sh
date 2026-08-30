#!/usr/bin/env bash
# A ultima hipotese ainda de pe sobre por que o tronco nao transfere.
#
# Os quatro testes de tronco rodaram todos com `--lr-tronco 1.0` (o padrao). Ou
# seja: carrega 4h de portugues e passa 12 epocas por cima com 16.917 exemplos
# feitos de 67 valores distintos. Se o tronco esta sendo SOBRESCRITO antes de
# servir, o efeito medido seria exatamente o que se viu — LM 64% melhor, agente
# parado.
#
# Referencia, mesmas sementes, mesmo tronco, lr-tronco 1.0:  107, 111, 116
# Controle 12M do zero, mesmas sementes:                     104, 115, 109
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
LOG=tronco_congelado.log
diga() { echo "[$(date +%H:%M:%S)] $*" | tee -a "$LOG"; }

until grep -q SEMENTES_PRONTAS sementes_extras.log 2>/dev/null; do sleep 120; done
diga "=== fila anterior terminou, comecando ==="

onda() {
  local lr="$1"; shift
  local rot="tr$(echo "$lr" | tr -d '.')"
  local pids=() s
  for s in "$@"; do
    ./target/release/teka.exe agente --epocas 12 --exemplos 16000 --semente "$s" \
      --cerebro lm_grande.bin --lr-tronco "$lr" --threads 4 \
      --saida "teka_${rot}_s$s.bin" --benchmark > "${rot}_s$s.log" 2>&1 &
    pids+=($!)
  done
  diga "lr-tronco $lr, sementes [$*], 4 threads cada"
  local p; for p in "${pids[@]}"; do wait "$p"; done
  for s in "$@"; do
    diga "  lr-tronco $lr  s$s: $(grep -E 'ferramenta certa:' "${rot}_s$s.log" | tail -1 || echo FALHOU)"
  done
  rm -f "teka_${rot}"_s*.bin
}

diga "--- 1/2: tronco quase congelado (lr x0.1) ---"
onda 0.1 7 8 9
diga "--- 2/2: tronco congelado de vez (lr x0.0) ---"
onda 0.0 7 8 9
diga "=== fim ==="
echo TRONCO_PRONTO >> "$LOG"
