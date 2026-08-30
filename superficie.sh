#!/usr/bin/env bash
# Retreino com variacao de superficie: maiuscula, acento, pontuacao.
#
# POR QUE, em uma medicao:
#   45 frases de hora/memoria/disco, escritas por uma pessoa de fora
#     como ela escreveu           34/45   76%
#     minuscula, sem acento, sem "?"  45/45  100%
#   Isolado: "Quanta RAM ta usando?" -> perguntar
#            "Quanta ram ta usando?" -> memoria
#
# O modelo entendia as 45. Perdia 24 pontos para tres bytes em caixa alta, porque
# o treino tinha 0% de maiuscula e 0% de acento — e o benchmark tambem, entao ele
# nunca poderia ter achado isso.
#
# Preset PEQUENO de proposito: e a configuracao da linha de base (teka_bal_s*) e
# tem desvio entre sementes de 1,5 frase, contra 5,4 do 12M. Para medir um efeito
# esperado de ~24 pontos, a regua quieta vale mais que o modelo grande.
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
LOG=superficie.log
diga() { echo "[$(date +%H:%M:%S)] $*" | tee -a "$LOG"; }

until grep -q SEMENTES_PRONTAS sementes_extras.log 2>/dev/null; do sleep 120; done
diga "=== fila de sementes terminou ==="

# O teka.exe fica travado enquanto ha treino rodando; agora da para reconstruir.
diga "recompilando com a variacao de superficie"
cargo build --release >> "$LOG" 2>&1 || { diga "BUILD FALHOU"; exit 1; }

pids=()
for s in 7 8 9; do
  ./target/release/teka.exe agente --epocas 12 --exemplos 16000 --semente "$s" \
    --preset pequeno --threads 4 --saida "teka_sup_s$s.bin" --benchmark \
    > "sup_s$s.log" 2>&1 &
  pids+=($!)
done
diga "3 sementes rodando, preset pequeno, 4 threads cada"
for p in "${pids[@]}"; do wait "$p"; done

diga "--- regua antiga (150 frases minhas, tudo minusculo) ---"
for s in 7 8 9; do
  diga "  sup   s$s: $(grep -E 'ferramenta certa:' "sup_s$s.log" | tail -1)"
  diga "  base  s$s: $(grep -E 'ferramenta certa:' "bal_s$s.log" | tail -1)"
done

diga "--- regua nova (500 frases do John, como ele escreveu) ---"
python medir_john.py teka_sup_s9.bin --cru > john_sup_cru.log 2>&1
tail -8 john_sup_cru.log | tee -a "$LOG"

diga "=== fim ==="
echo SUPERFICIE_PRONTA >> "$LOG"
